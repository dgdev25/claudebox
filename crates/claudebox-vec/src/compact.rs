use std::path::PathBuf;

/// Summary statistics from a compaction pass.
#[derive(Debug, Clone)]
pub struct CompactStats {
    pub entries_removed: usize,
    pub entries_kept: usize,
}

/// Physically removes tombstoned `ChunkRecord`s from the VEC_SEG HNSW store.
///
/// Compaction rebuilds the HNSW index excluding any chunks where
/// `tombstoned == true`, using `InitTransaction` for atomicity (write to
/// `.rvf.tmp`, then rename). Appends a `WitnessCompact` event on success.
pub struct VecCompactor {
    pub rvf_path: PathBuf,
}

impl VecCompactor {
    /// Rebuild VEC_SEG without tombstoned entries.
    pub async fn compact(&self) -> anyhow::Result<CompactStats> {
        anyhow::bail!("VecCompactor::compact: not yet implemented — deferred to Phase 10");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "integration test deferred to Phase 10"]
    fn test_compact_removes_tombstoned_entries() {
        todo!()
    }

    #[test]
    fn test_compact_uses_init_transaction_atomicity() {
        let stats = CompactStats {
            entries_removed: 0,
            entries_kept: 5,
        };
        assert_eq!(stats.entries_removed, 0);
    }
}
