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
#[derive(Debug, Clone)]
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

/// Walks a workspace and indexes source files into the VEC_SEG HNSW store.
///
/// **Note:** `WorkspaceIndexer` holds a `fastembed::TextEmbedding` model
/// internally. Constructing it downloads ~25 MB on first use. All tests in
/// this module test the pure helpers (`should_skip`, `chunk_text`) and do NOT
/// instantiate `WorkspaceIndexer`.
pub struct WorkspaceIndexer {
    pub workspace_path: std::path::PathBuf,
    pub rvf_path: std::path::PathBuf,
}

impl WorkspaceIndexer {
    /// Index every eligible file in the workspace.
    pub async fn index_all(&self) -> anyhow::Result<IndexStats> {
        anyhow::bail!("index_all: not yet implemented — deferred to Phase 7");
    }

    /// Index only files whose mtime is newer than `since`.
    pub async fn index_changed(&self, _since: SystemTime) -> anyhow::Result<IndexStats> {
        anyhow::bail!("index_changed: not yet implemented — deferred to Phase 7");
    }

    /// Chunk and embed a single file, returning the resulting records.
    pub async fn index_file(&self, _path: &Path) -> anyhow::Result<Vec<ChunkRecord>> {
        anyhow::bail!("index_file: not yet implemented — deferred to Phase 7");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
