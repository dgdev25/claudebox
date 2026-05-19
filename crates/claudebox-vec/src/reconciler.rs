use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::indexer::{should_skip, ChunkRecord};

/// Summary statistics from a reconciliation pass.
#[derive(Debug, Clone)]
pub struct ReconcileStats {
    pub files_checked: usize,
    pub files_tombstoned: usize,
}

/// Boot-time reconciler: marks `ChunkRecord`s as tombstoned when their source
/// file no longer exists on disk. Physical removal is deferred to `VecCompactor`.
pub struct VecReconciler {
    pub workspace_path: PathBuf,
    pub rvf_path: PathBuf,
}

impl VecReconciler {
    /// Fast metadata scan — does NOT recompute embeddings.
    ///
    /// Reads `ChunkRecord`s from the `<rvf>.chunks.jsonl` sidecar, walks the
    /// workspace to collect existing files (filtered by `should_skip`), calls
    /// the pure `reconcile_chunks` to mark missing files as tombstoned, writes
    /// the updated list back, and appends a `VecReconcile` witness event with
    /// the number of files tombstoned.
    ///
    /// Returns zero-stats and creates no files when the sidecar is absent.
    pub async fn reconcile(&self) -> anyhow::Result<ReconcileStats> {
        let sidecar = chunks_sidecar_path(&self.rvf_path);
        let chunks = read_chunks(&sidecar)?;
        if chunks.is_empty() {
            return Ok(ReconcileStats { files_checked: 0, files_tombstoned: 0 });
        }

        let existing = collect_existing_files(&self.workspace_path)?;
        let (updated, stats) = reconcile_chunks(chunks, &existing);

        write_chunks(&sidecar, &updated)?;

        if stats.files_tombstoned > 0 {
            claudebox_core::witness::append_witness_entry(
                &self.rvf_path,
                claudebox_witness::WitnessEvent::VecReconcile {
                    files_removed: stats.files_tombstoned as u32,
                },
            )?;
        }
        Ok(stats)
    }
}

/// Path of the `ChunkRecord` JSONL sidecar for a given `.rvf` file.
pub fn chunks_sidecar_path(rvf_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.chunks.jsonl", rvf_path.display()))
}

fn read_chunks(path: &Path) -> anyhow::Result<Vec<ChunkRecord>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: ChunkRecord = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("corrupt chunk record: {e}"))?;
        out.push(rec);
    }
    Ok(out)
}

fn write_chunks(path: &Path, chunks: &[ChunkRecord]) -> anyhow::Result<()> {
    let mut lines = Vec::with_capacity(chunks.len());
    for c in chunks {
        lines.push(
            serde_json::to_string(c)
                .map_err(|e| anyhow::anyhow!("chunk serialisation failed: {e}"))?,
        );
    }
    std::fs::write(path, lines.join("\n") + "\n")
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))
}

/// Walk `workspace_root` recursively and return the set of file paths
/// (relative, forward-slash-separated) that are eligible for indexing.
fn collect_existing_files(workspace_root: &Path) -> anyhow::Result<HashSet<String>> {
    let mut out = HashSet::new();
    let mut stack = vec![workspace_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if should_skip(&rel) {
                continue;
            }
            out.insert(rel);
        }
    }
    Ok(out)
}

/// Pure function: for each chunk whose `file_path` is absent from `existing`,
/// sets `tombstoned = true`. Returns the updated chunk list and summary stats.
///
/// `files_checked` counts the number of *unique* file paths seen across all
/// chunks. `files_tombstoned` counts how many of those unique paths were not
/// found in `existing`.
pub fn reconcile_chunks(
    mut chunks: Vec<ChunkRecord>,
    existing: &HashSet<String>,
) -> (Vec<ChunkRecord>, ReconcileStats) {
    // Collect unique file paths as owned Strings to avoid lifetime conflicts
    // when we later take a mutable borrow over the same vec.
    let seen_files: HashSet<String> = chunks
        .iter()
        .map(|c| c.file_path.clone())
        .collect();

    let tombstoned_files: HashSet<&String> = seen_files
        .iter()
        .filter(|fp| !existing.contains(fp.as_str()))
        .collect();

    let files_checked = seen_files.len();
    let files_tombstoned = tombstoned_files.len();

    // Mutate chunks: tombstone those whose file is no longer on disk.
    for chunk in &mut chunks {
        if tombstoned_files.contains(&chunk.file_path) {
            chunk.tombstoned = true;
        }
    }

    let stats = ReconcileStats {
        files_checked,
        files_tombstoned,
    };

    (chunks, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::ChunkRecord;

    #[test]
    fn test_reconcile_chunks_tombstones_missing_file() {
        let existing_files: HashSet<String> =
            vec!["src/a.rs".to_string()].into_iter().collect();

        let chunks = vec![
            ChunkRecord {
                file_path: "src/a.rs".into(),
                chunk_index: 0,
                text: "fn foo".into(),
                embedding: vec![],
                tombstoned: false,
            },
            ChunkRecord {
                file_path: "src/a.rs".into(),
                chunk_index: 1,
                text: "fn bar".into(),
                embedding: vec![],
                tombstoned: false,
            },
            ChunkRecord {
                file_path: "src/deleted.rs".into(),
                chunk_index: 0,
                text: "fn gone".into(),
                embedding: vec![],
                tombstoned: false,
            },
        ];

        let (updated, stats) = reconcile_chunks(chunks, &existing_files);

        assert!(!updated[0].tombstoned, "src/a.rs chunk 0 should not be tombstoned");
        assert!(!updated[1].tombstoned, "src/a.rs chunk 1 should not be tombstoned");
        assert!(updated[2].tombstoned, "src/deleted.rs chunk should be tombstoned");
        assert_eq!(stats.files_tombstoned, 1);
        assert_eq!(stats.files_checked, 2);
    }

    #[test]
    fn test_reconcile_chunks_noop_when_all_files_exist() {
        let existing_files: HashSet<String> =
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()].into_iter().collect();
        let chunks = vec![
            ChunkRecord {
                file_path: "src/a.rs".into(),
                chunk_index: 0,
                text: "fn a".into(),
                embedding: vec![],
                tombstoned: false,
            },
            ChunkRecord {
                file_path: "src/b.rs".into(),
                chunk_index: 0,
                text: "fn b".into(),
                embedding: vec![],
                tombstoned: false,
            },
        ];
        let (updated, stats) = reconcile_chunks(chunks, &existing_files);
        assert!(updated.iter().all(|c| !c.tombstoned));
        assert_eq!(stats.files_tombstoned, 0);
    }

    #[tokio::test]
    async fn test_reconcile_returns_zero_stats_when_sidecar_absent() {
        let dir = tempfile::tempdir().unwrap();
        let reconciler = VecReconciler {
            workspace_path: dir.path().to_path_buf(),
            rvf_path: dir.path().join("nonexistent.rvf"),
        };
        let stats = reconciler.reconcile().await.unwrap();
        assert_eq!(stats.files_checked, 0);
        assert_eq!(stats.files_tombstoned, 0);
    }

    #[tokio::test]
    async fn test_reconcile_tombstones_missing_files_and_writes_back() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("a.rs"), b"fn a() {}").unwrap();

        let rvf = dir.path().join("app.rvf");
        let sidecar = chunks_sidecar_path(&rvf);
        let chunks = vec![
            ChunkRecord {
                file_path: "a.rs".into(),
                chunk_index: 0,
                text: "fn a".into(),
                embedding: vec![],
                tombstoned: false,
            },
            ChunkRecord {
                file_path: "gone.rs".into(),
                chunk_index: 0,
                text: "fn gone".into(),
                embedding: vec![],
                tombstoned: false,
            },
        ];
        write_chunks(&sidecar, &chunks).unwrap();

        let reconciler = VecReconciler { workspace_path: workspace, rvf_path: rvf };
        let stats = reconciler.reconcile().await.unwrap();
        assert_eq!(stats.files_checked, 2);
        assert_eq!(stats.files_tombstoned, 1);

        let after = read_chunks(&sidecar).unwrap();
        let by_path: std::collections::HashMap<_, _> =
            after.iter().map(|c| (c.file_path.as_str(), c.tombstoned)).collect();
        assert_eq!(by_path["a.rs"], false);
        assert_eq!(by_path["gone.rs"], true);
    }

    #[test]
    fn test_reconcile_stats_counts_correctly() {
        let stats = ReconcileStats {
            files_checked: 10,
            files_tombstoned: 3,
        };
        assert_eq!(stats.files_tombstoned, 3);
        assert_eq!(stats.files_checked - stats.files_tombstoned, 7);
    }
}
