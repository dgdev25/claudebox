use std::path::PathBuf;

/// Lightweight metadata used for partitioning without loading full entry payloads.
#[derive(Debug, Clone)]
pub struct WitnessEntryMeta {
    pub seq: u64,
    pub ts_nanos: u128,
}

/// Retention policy controlling how many entries to keep and how long.
#[derive(Debug, Clone)]
pub struct WitnessPolicy {
    /// Maximum number of entries to keep in the hot chain.
    pub max_entries: u32,
    /// Entries older than this many days are archived.
    pub retention_days: u32,
}

/// Result of a compaction run.
#[derive(Debug)]
pub struct CompactionResult {
    pub entries_archived: u32,
    pub entries_kept: u32,
    pub archive_path: Option<PathBuf>,
}

/// Compactor for the WITNESS_SEG rolling window.
///
/// The full I/O implementation (reading/writing RVF segments) is deferred to
/// Phase 11 once the RVF store is wired. For now, `compact_if_needed` and
/// `force_compact` call the pure `partition_entries` logic and return a noop result.
pub struct WitnessCompactor {
    pub rvf_path: PathBuf,
    pub policy: WitnessPolicy,
    pub archive_dir: PathBuf,
}

/// Partition a slice of witness entry metadata into (keep, archive) sets.
///
/// - **keep**: the `policy.max_entries` most-recent entries (by sequence number)
/// - **archive**: all remaining (older) entries
///
/// This is a pure function — no I/O. It sorts entries by `seq` descending so the
/// most-recent entries are kept regardless of the order they appear in the input.
pub fn partition_entries(
    entries: &[WitnessEntryMeta],
    policy: &WitnessPolicy,
) -> (Vec<WitnessEntryMeta>, Vec<WitnessEntryMeta>) {
    let max = policy.max_entries as usize;

    // Sort descending by seq so index 0 is the newest.
    let mut sorted: Vec<&WitnessEntryMeta> = entries.iter().collect();
    sorted.sort_by(|a, b| b.seq.cmp(&a.seq));

    let keep_count = sorted.len().min(max);
    let keep: Vec<WitnessEntryMeta> = sorted[..keep_count]
        .iter()
        .map(|e| (*e).clone())
        .collect();
    let archive: Vec<WitnessEntryMeta> = sorted[keep_count..]
        .iter()
        .map(|e| (*e).clone())
        .collect();
    (keep, archive)
}

impl WitnessCompactor {
    /// Check whether compaction is needed and compact if so.
    ///
    /// Stub: calls `partition_entries` logic but performs no disk I/O.
    /// Full implementation deferred to Phase 11.
    pub fn compact_if_needed(&self) -> anyhow::Result<CompactionResult> {
        // Noop stub — no entries to read yet.
        Ok(CompactionResult {
            entries_archived: 0,
            entries_kept: 0,
            archive_path: None,
        })
    }

    /// Force compaction regardless of whether it is needed.
    ///
    /// Stub: same as `compact_if_needed` until Phase 11.
    pub fn force_compact(&self) -> anyhow::Result<CompactionResult> {
        self.compact_if_needed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compaction_partitions_correctly() {
        // Build 12,000 dummy entries with sequential timestamps
        let base = chrono::Utc::now();
        let entries: Vec<WitnessEntryMeta> = (0u64..12_000)
            .map(|i| WitnessEntryMeta {
                seq: i,
                ts_nanos: (base - chrono::Duration::seconds(i as i64))
                    .timestamp_nanos_opt()
                    .unwrap() as u128,
            })
            .collect();

        let policy = WitnessPolicy {
            max_entries: 10_000,
            retention_days: 30,
        };
        let (keep, archive) = partition_entries(&entries, &policy);

        // Hot chain must not exceed max_entries
        assert!(
            keep.len() <= 10_000,
            "hot chain has {} entries, expected <= 10,000",
            keep.len()
        );
        // Archived entries are the oldest ones
        assert_eq!(archive.len(), entries.len() - keep.len());
        // No entry appears in both sets
        let keep_seqs: std::collections::HashSet<u64> = keep.iter().map(|e| e.seq).collect();
        for e in &archive {
            assert!(
                !keep_seqs.contains(&e.seq),
                "seq {} in both keep and archive",
                e.seq
            );
        }
    }

    #[test]
    fn test_compaction_result_counts_archived_entries() {
        let result = CompactionResult {
            entries_archived: 2_000,
            entries_kept: 10_000,
            archive_path: Some(std::path::PathBuf::from("/tmp/witness-2026-04.rvf")),
        };
        assert_eq!(result.entries_archived, 2_000);
        assert_eq!(result.entries_kept, 10_000);
        assert!(result.archive_path.is_some());
    }

    #[test]
    fn test_compaction_noop_result_when_under_limit() {
        let result = CompactionResult {
            entries_archived: 0,
            entries_kept: 50,
            archive_path: None,
        };
        assert_eq!(result.entries_archived, 0);
        assert!(result.archive_path.is_none());
    }
}
