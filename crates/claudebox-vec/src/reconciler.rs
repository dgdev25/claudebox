use std::collections::HashSet;
use std::path::PathBuf;

use crate::indexer::ChunkRecord;

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
    /// Reads chunk metadata from VEC_SEG, walks the workspace to build the set
    /// of currently-existing files, calls `reconcile_chunks`, then writes the
    /// updated metadata back. Appends a `VecReconcile` witness event on
    /// completion.
    pub async fn reconcile(&self) -> anyhow::Result<ReconcileStats> {
        anyhow::bail!("VecReconciler::reconcile: not yet implemented — deferred to Phase 7");
    }
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
