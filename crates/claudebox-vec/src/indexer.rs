use std::path::Path;
use std::time::SystemTime;

/// Path components and extensions that should be excluded from indexing.
/// NOTE: fastembed (TextEmbedding) is NOT instantiated in tests to avoid
/// downloading the ~25 MB model. The pure functions `should_skip` and
/// `chunk_text` are tested independently.
pub static SKIP_PATTERNS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    ".venv",
    "dist",
    "build",
    ".next",
    ".claudebox",
];

pub static SKIP_EXTENSIONS: &[&str] = &[
    "lock", "bin", "png", "jpg", "jpeg", "gif", "svg", "ico", "mp4", "webm", "wav", "mp3",
    "wasm", "rvf",
];

/// Returns `true` if the given path string should be excluded from indexing.
///
/// Checks every path component against `SKIP_PATTERNS` and the file extension
/// against `SKIP_EXTENSIONS`. Both checks are purely string-based — no disk I/O.
pub fn should_skip(path: &str) -> bool {
    let p = Path::new(path);

    // Check each component against skip patterns.
    for component in p.components() {
        let s = component.as_os_str().to_string_lossy();
        for &pattern in SKIP_PATTERNS {
            if s == pattern {
                return true;
            }
        }
    }

    // Check the file extension against the binary/image list.
    if let Some(ext) = p.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        if SKIP_EXTENSIONS.contains(&ext_lower.as_ref()) {
            return true;
        }
    }

    false
}

/// Splits `text` into overlapping chunks using a sliding window over whitespace-
/// delimited tokens (words).
///
/// * `chunk_tokens`   — number of words per chunk.
/// * `overlap_tokens` — number of words shared between consecutive chunks.
///
/// Each chunk starts at `previous_start + chunk_tokens - overlap_tokens`. The
/// final chunk may contain fewer than `chunk_tokens` words when the text is
/// exhausted.
pub fn chunk_text(text: &str, chunk_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() || chunk_tokens == 0 {
        return vec![];
    }

    let step = if chunk_tokens > overlap_tokens {
        chunk_tokens - overlap_tokens
    } else {
        1
    };

    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < tokens.len() {
        let end = (start + chunk_tokens).min(tokens.len());
        chunks.push(tokens[start..end].join(" "));
        if end == tokens.len() {
            break;
        }
        start += step;
    }

    chunks
}

/// A single embedded chunk of a source file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChunkRecord {
    pub file_path: String,
    pub chunk_index: usize,
    pub text: String,
    pub embedding: Vec<f32>,
    pub tombstoned: bool,
}

/// Summary statistics returned by indexing operations.
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub chunks_created: usize,
    pub files_tombstoned: usize,
    pub elapsed_ms: u64,
}

/// Walks a workspace and indexes source files, persisting `ChunkRecord`s
/// to the `<rvf>.chunks.jsonl` sidecar.
///
/// **Embeddings:** The chunking + sidecar pipeline runs unconditionally and
/// produces `ChunkRecord { embedding: vec![], .. }` by default. Wiring an
/// actual embedder (e.g. `fastembed::TextEmbedding`) is gated behind the
/// `embed` feature so unit tests don't pull in ONNX Runtime or download
/// the ~25 MB model. MCP `search_codebase` falls back to substring search
/// when embeddings are empty (see `mcp_server::handle_search_codebase`).
pub struct WorkspaceIndexer {
    pub workspace_path: std::path::PathBuf,
    pub rvf_path: std::path::PathBuf,
    /// Tokens per chunk. Default `512` — small enough for fastembed's
    /// MiniLM 256-token window after BPE tokenisation overhead.
    pub chunk_tokens: usize,
    pub overlap_tokens: usize,
}

impl WorkspaceIndexer {
    /// Construct an indexer with default chunking parameters.
    pub fn new(workspace_path: std::path::PathBuf, rvf_path: std::path::PathBuf) -> Self {
        Self { workspace_path, rvf_path, chunk_tokens: 512, overlap_tokens: 64 }
    }

    /// Walk the workspace, index every eligible file, persist the resulting
    /// chunks to the sidecar (replacing prior entries for re-indexed files).
    pub async fn index_all(&self) -> anyhow::Result<IndexStats> {
        let start = std::time::Instant::now();
        let mut all = Vec::new();
        let mut files_indexed = 0usize;
        for path in walk_workspace(&self.workspace_path) {
            let chunks = self.index_file(&path).await?;
            if !chunks.is_empty() {
                files_indexed += 1;
                all.extend(chunks);
            }
        }
        let chunks_created = all.len();
        merge_into_sidecar(&self.rvf_path, all)?;
        Ok(IndexStats {
            files_indexed,
            chunks_created,
            files_tombstoned: 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Walk the workspace, indexing only files whose `mtime` is newer than
    /// `since`. The sidecar is updated incrementally — unchanged files keep
    /// their existing chunks untouched.
    pub async fn index_changed(&self, since: SystemTime) -> anyhow::Result<IndexStats> {
        let start = std::time::Instant::now();
        let mut all = Vec::new();
        let mut files_indexed = 0usize;
        for path in walk_workspace(&self.workspace_path) {
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            if mtime.is_none_or(|t| t <= since) {
                continue;
            }
            let chunks = self.index_file(&path).await?;
            if !chunks.is_empty() {
                files_indexed += 1;
                all.extend(chunks);
            }
        }
        let chunks_created = all.len();
        merge_into_sidecar(&self.rvf_path, all)?;
        Ok(IndexStats {
            files_indexed,
            chunks_created,
            files_tombstoned: 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Read `path`, skip via `should_skip`, chunk the text, and return one
    /// `ChunkRecord` per chunk. Files that fail to read as UTF-8 or are
    /// flagged by `should_skip` produce an empty result rather than an error.
    pub async fn index_file(&self, path: &Path) -> anyhow::Result<Vec<ChunkRecord>> {
        let rel = path
            .strip_prefix(&self.workspace_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if should_skip(&rel) {
            return Ok(vec![]);
        }
        let bytes = match tokio::fs::read(path).await {
            Ok(b) => b,
            Err(_) => return Ok(vec![]),
        };
        let text = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]),
        };
        let chunks = chunk_text(&text, self.chunk_tokens, self.overlap_tokens);
        Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(i, t)| ChunkRecord {
                file_path: rel.clone(),
                chunk_index: i,
                text: t,
                embedding: vec![],
                tombstoned: false,
            })
            .collect())
    }
}

/// Recursively walk `root`, yielding regular files only.
fn walk_workspace(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ft = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                out.push(path);
            }
        }
    }
    out
}

/// Merge `new_chunks` into `<rvf>.chunks.jsonl`: for every file path present
/// in `new_chunks`, the prior sidecar entries for that file are dropped and
/// replaced with the new chunks. Other files' chunks are preserved.
fn merge_into_sidecar(
    rvf_path: &Path,
    new_chunks: Vec<ChunkRecord>,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    let sidecar = crate::reconciler::chunks_sidecar_path(rvf_path);

    let existing = if sidecar.exists() {
        let content = std::fs::read_to_string(&sidecar)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", sidecar.display()))?;
        let mut out = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            out.push(
                serde_json::from_str::<ChunkRecord>(line)
                    .map_err(|e| anyhow::anyhow!("corrupt chunk record: {e}"))?,
            );
        }
        out
    } else {
        Vec::new()
    };

    let replaced: HashSet<String> =
        new_chunks.iter().map(|c| c.file_path.clone()).collect();
    let mut merged: Vec<ChunkRecord> = existing
        .into_iter()
        .filter(|c| !replaced.contains(&c.file_path))
        .collect();
    merged.extend(new_chunks);

    let tmp = std::path::PathBuf::from(format!("{}.tmp", sidecar.display()));
    let mut lines = Vec::with_capacity(merged.len());
    for c in &merged {
        lines.push(
            serde_json::to_string(c)
                .map_err(|e| anyhow::anyhow!("chunk serialisation failed: {e}"))?,
        );
    }
    std::fs::write(&tmp, lines.join("\n") + "\n")
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &sidecar).map_err(|e| {
        anyhow::anyhow!("rename {} -> {} failed: {e}", tmp.display(), sidecar.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_index_file_skips_filtered_paths() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        std::fs::create_dir_all(workspace.join("node_modules")).unwrap();
        let skipped = workspace.join("node_modules").join("dep.js");
        std::fs::write(&skipped, "var x = 1;").unwrap();

        let indexer = WorkspaceIndexer::new(workspace, dir.path().join("rvf.rvf"));
        let chunks = indexer.index_file(&skipped).await.unwrap();
        assert!(chunks.is_empty(), "node_modules paths should be skipped");
    }

    #[tokio::test]
    async fn test_index_file_chunks_text_and_emits_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let file = workspace.join("src/main.rs");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(&file, "fn main() { println!(\"hello world\"); }").unwrap();

        let indexer = WorkspaceIndexer::new(workspace, dir.path().join("rvf.rvf"));
        let chunks = indexer.index_file(&file).await.unwrap();
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].file_path, "src/main.rs");
        assert!(chunks[0].text.contains("fn main"));
        assert!(chunks[0].embedding.is_empty(), "embedding is empty without the `embed` feature");
    }

    #[tokio::test]
    async fn test_index_all_walks_recursively_and_writes_sidecar() {
        use crate::reconciler::chunks_sidecar_path;
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(workspace.join("src/nested")).unwrap();
        std::fs::write(workspace.join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(workspace.join("src/nested/b.rs"), "fn b() {}").unwrap();
        std::fs::write(workspace.join("README.md"), "# project").unwrap();
        // Should be skipped:
        std::fs::create_dir_all(workspace.join("target")).unwrap();
        std::fs::write(workspace.join("target/skip.rs"), "fn no() {}").unwrap();

        let rvf = dir.path().join("app.rvf");
        let indexer = WorkspaceIndexer::new(workspace, rvf.clone());
        let stats = indexer.index_all().await.unwrap();
        assert_eq!(stats.files_indexed, 3);
        assert!(stats.chunks_created >= 3);

        let sidecar = chunks_sidecar_path(&rvf);
        assert!(sidecar.exists());
        let content = std::fs::read_to_string(&sidecar).unwrap();
        assert!(content.contains("\"src/a.rs\""));
        assert!(content.contains("\"src/nested/b.rs\""));
        assert!(content.contains("\"README.md\""));
        assert!(!content.contains("target/skip.rs"), "target/ must be skipped");
    }

    #[tokio::test]
    async fn test_index_all_merge_replaces_chunks_for_reindexed_files() {
        use crate::reconciler::chunks_sidecar_path;
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("a.rs"), "fn old() {}").unwrap();

        let rvf = dir.path().join("app.rvf");
        let indexer = WorkspaceIndexer::new(workspace.clone(), rvf.clone());
        indexer.index_all().await.unwrap();

        std::fs::write(workspace.join("a.rs"), "fn brand_new_function() {}").unwrap();
        indexer.index_all().await.unwrap();

        let sidecar = chunks_sidecar_path(&rvf);
        let content = std::fs::read_to_string(&sidecar).unwrap();
        assert!(content.contains("brand_new_function"), "new content must be present");
        assert!(!content.contains("fn old"), "stale chunk for re-indexed file must be replaced");
    }

    #[tokio::test]
    async fn test_index_changed_only_picks_up_modified_files() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("old.rs"), "fn old() {}").unwrap();
        // Sleep to ensure mtime ordering on filesystems with second-resolution mtime.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let cutoff = SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(workspace.join("new.rs"), "fn brand_new() {}").unwrap();

        let rvf = dir.path().join("app.rvf");
        let indexer = WorkspaceIndexer::new(workspace, rvf);
        let stats = indexer.index_changed(cutoff).await.unwrap();
        assert_eq!(stats.files_indexed, 1, "only new.rs should be indexed");
    }

    #[test]
    fn test_skip_patterns_exclude_binary_paths() {
        assert!(should_skip("node_modules/express/index.js"));
        assert!(should_skip(".git/config"));
        assert!(should_skip("target/debug/binary"));
        assert!(should_skip("assets/logo.png"));
        assert!(should_skip("dist/bundle.js"));
        assert!(!should_skip("src/main.rs"));
        assert!(!should_skip("lib/utils.py"));
    }

    #[test]
    fn test_chunk_windowing_produces_correct_boundaries() {
        let text = "word ".repeat(1024);
        let chunks = chunk_text(&text, 512, 64);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn test_index_stats_counts_files() {
        let stats = IndexStats {
            files_indexed: 5,
            chunks_created: 42,
            files_tombstoned: 0,
            elapsed_ms: 100,
        };
        assert_eq!(stats.files_indexed, 5);
    }
}
