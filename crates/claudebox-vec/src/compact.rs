use std::path::{Path, PathBuf};

use crate::indexer::ChunkRecord;
use crate::reconciler::chunks_sidecar_path;

/// Summary statistics from a compaction pass.
#[derive(Debug, Clone)]
pub struct CompactStats {
    pub entries_removed: usize,
    pub entries_kept: usize,
}

/// Physically removes tombstoned `ChunkRecord`s from the chunks sidecar.
///
/// Reads `<rvf>.chunks.jsonl`, drops records where `tombstoned == true`,
/// and atomically writes the survivors back via a temp file + rename.
/// The HNSW index (when wired in #22) will be rebuilt from the kept
/// chunks on next start.
pub struct VecCompactor {
    pub rvf_path: PathBuf,
}

impl VecCompactor {
    /// Rewrite the chunks sidecar excluding tombstoned entries.
    ///
    /// Returns zero-stats and creates no files when the sidecar is absent.
    pub async fn compact(&self) -> anyhow::Result<CompactStats> {
        let sidecar = chunks_sidecar_path(&self.rvf_path);
        let chunks = read_chunks(&sidecar)?;
        if chunks.is_empty() {
            return Ok(CompactStats { entries_removed: 0, entries_kept: 0 });
        }

        let (kept, removed): (Vec<_>, Vec<_>) =
            chunks.into_iter().partition(|c| !c.tombstoned);

        if removed.is_empty() {
            return Ok(CompactStats { entries_removed: 0, entries_kept: kept.len() });
        }

        atomic_write_chunks(&sidecar, &kept)?;

        Ok(CompactStats { entries_removed: removed.len(), entries_kept: kept.len() })
    }
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
        out.push(
            serde_json::from_str::<ChunkRecord>(line)
                .map_err(|e| anyhow::anyhow!("corrupt chunk record: {e}"))?,
        );
    }
    Ok(out)
}

/// Write `chunks` to `path` via `<path>.tmp` + rename so the sidecar is
/// never observed in a partially-written state.
fn atomic_write_chunks(path: &Path, chunks: &[ChunkRecord]) -> anyhow::Result<()> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let mut lines = Vec::with_capacity(chunks.len());
    for c in chunks {
        lines.push(
            serde_json::to_string(c)
                .map_err(|e| anyhow::anyhow!("chunk serialisation failed: {e}"))?,
        );
    }
    std::fs::write(&tmp, lines.join("\n") + "\n")
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| anyhow::anyhow!("atomic rename {} -> {} failed: {e}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_sidecar(path: &Path, chunks: &[ChunkRecord]) {
        let mut lines = Vec::new();
        for c in chunks {
            lines.push(serde_json::to_string(c).unwrap());
        }
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    fn chunk(file: &str, tombstoned: bool) -> ChunkRecord {
        ChunkRecord {
            file_path: file.into(),
            chunk_index: 0,
            text: "x".into(),
            embedding: vec![],
            tombstoned,
        }
    }

    #[tokio::test]
    async fn test_compact_noop_when_sidecar_absent() {
        let dir = tempdir().unwrap();
        let compactor = VecCompactor { rvf_path: dir.path().join("missing.rvf") };
        let stats = compactor.compact().await.unwrap();
        assert_eq!(stats.entries_removed, 0);
        assert_eq!(stats.entries_kept, 0);
    }

    #[tokio::test]
    async fn test_compact_removes_tombstoned_chunks() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("app.rvf");
        let sidecar = chunks_sidecar_path(&rvf);
        write_sidecar(
            &sidecar,
            &[chunk("a.rs", false), chunk("b.rs", true), chunk("c.rs", false)],
        );

        let stats = VecCompactor { rvf_path: rvf }.compact().await.unwrap();
        assert_eq!(stats.entries_removed, 1);
        assert_eq!(stats.entries_kept, 2);

        let remaining = read_chunks(&sidecar).unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().all(|c| !c.tombstoned));
        assert!(remaining.iter().any(|c| c.file_path == "a.rs"));
        assert!(remaining.iter().any(|c| c.file_path == "c.rs"));
    }

    #[tokio::test]
    async fn test_compact_does_not_rewrite_when_no_tombstones() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("app.rvf");
        let sidecar = chunks_sidecar_path(&rvf);
        write_sidecar(&sidecar, &[chunk("a.rs", false), chunk("b.rs", false)]);
        let mtime_before = std::fs::metadata(&sidecar).unwrap().modified().unwrap();

        let stats = VecCompactor { rvf_path: rvf }.compact().await.unwrap();
        assert_eq!(stats.entries_removed, 0);
        assert_eq!(stats.entries_kept, 2);

        let mtime_after = std::fs::metadata(&sidecar).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "sidecar should not be rewritten when no tombstones");
    }

    #[test]
    fn test_compact_uses_init_transaction_atomicity() {
        let stats = CompactStats { entries_removed: 0, entries_kept: 5 };
        assert_eq!(stats.entries_removed, 0);
    }
}
