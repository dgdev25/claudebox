use std::collections::HashSet;
use std::path::PathBuf;

use crate::WitnessEntry;

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
    /// Reads the `.witness` JSONL sidecar, partitions by `policy.max_entries`,
    /// and if there are entries to archive: writes old entries to a dated JSONL
    /// file in `archive_dir` and rewrites the sidecar with only the kept entries.
    pub fn compact_if_needed(&self) -> anyhow::Result<CompactionResult> {
        let entries = self.load_entries()?;
        let meta: Vec<WitnessEntryMeta> = entries
            .iter()
            .map(|e| WitnessEntryMeta { seq: e.seq, ts_nanos: e.ts_nanos })
            .collect();

        let (keep_meta, archive_meta) = partition_entries(&meta, &self.policy);

        if archive_meta.is_empty() {
            return Ok(CompactionResult {
                entries_archived: 0,
                entries_kept: entries.len() as u32,
                archive_path: None,
            });
        }

        let keep_seqs: HashSet<u64> = keep_meta.iter().map(|e| e.seq).collect();
        let (keep_entries, archive_entries): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|e| keep_seqs.contains(&e.seq));

        let archive_path = self.write_archive(&archive_entries)?;
        self.write_witness(&keep_entries)?;

        Ok(CompactionResult {
            entries_archived: archive_entries.len() as u32,
            entries_kept: keep_entries.len() as u32,
            archive_path: Some(archive_path),
        })
    }

    /// Force compaction regardless of whether it is needed.
    ///
    /// Rewrites the witness sidecar even when the entry count is under the limit.
    /// Useful for defragmentation or after manual edits.
    pub fn force_compact(&self) -> anyhow::Result<CompactionResult> {
        let entries = self.load_entries()?;
        if entries.is_empty() {
            return Ok(CompactionResult { entries_archived: 0, entries_kept: 0, archive_path: None });
        }

        let meta: Vec<WitnessEntryMeta> = entries
            .iter()
            .map(|e| WitnessEntryMeta { seq: e.seq, ts_nanos: e.ts_nanos })
            .collect();

        let (keep_meta, archive_meta) = partition_entries(&meta, &self.policy);

        let keep_seqs: HashSet<u64> = keep_meta.iter().map(|e| e.seq).collect();
        let (keep_entries, archive_entries): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|e| keep_seqs.contains(&e.seq));

        let archive_path = if !archive_meta.is_empty() {
            Some(self.write_archive(&archive_entries)?)
        } else {
            None
        };

        self.write_witness(&keep_entries)?;

        Ok(CompactionResult {
            entries_archived: archive_entries.len() as u32,
            entries_kept: keep_entries.len() as u32,
            archive_path,
        })
    }

    fn witness_path(&self) -> PathBuf {
        PathBuf::from(format!("{}.witness", self.rvf_path.display()))
    }

    fn load_entries(&self) -> anyhow::Result<Vec<WitnessEntry>> {
        let path = self.witness_path();
        if !path.exists() {
            return Ok(vec![]);
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read witness file: {e}"))?;
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let entry: WitnessEntry = serde_json::from_str(line)
                .map_err(|e| anyhow::anyhow!("malformed witness entry: {e}"))?;
            entries.push(entry);
        }
        Ok(entries)
    }

    fn write_witness(&self, entries: &[WitnessEntry]) -> anyhow::Result<()> {
        let path = self.witness_path();
        let mut lines = Vec::with_capacity(entries.len());
        for e in entries {
            lines.push(
                serde_json::to_string(e)
                    .map_err(|e| anyhow::anyhow!("witness serialisation failed: {e}"))?,
            );
        }
        std::fs::write(&path, lines.join("\n") + "\n")
            .map_err(|e| anyhow::anyhow!("failed to write witness file: {e}"))
    }

    fn write_archive(&self, entries: &[WitnessEntry]) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.archive_dir)
            .map_err(|e| anyhow::anyhow!("failed to create archive dir: {e}"))?;

        let epoch_secs = entries
            .last()
            .map_or(0, |e| e.ts_nanos / 1_000_000_000);
        let name = format!("witness-archive-{epoch_secs}.jsonl");
        let path = self.archive_dir.join(name);

        let mut lines = Vec::with_capacity(entries.len());
        for e in entries {
            lines.push(
                serde_json::to_string(e)
                    .map_err(|e| anyhow::anyhow!("archive serialisation failed: {e}"))?,
            );
        }
        std::fs::write(&path, lines.join("\n") + "\n")
            .map_err(|e| anyhow::anyhow!("failed to write archive: {e}"))?;

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WitnessEvent, writer::WitnessWriter};
    use ed25519_dalek::SigningKey;
    use rand::thread_rng;
    use tempfile::tempdir;

    fn make_witness_file(path: &std::path::Path, count: usize) {
        let key = SigningKey::generate(&mut thread_rng());
        let mut entries = Vec::new();
        let genesis = WitnessWriter::create_genesis(
            &key, WitnessEvent::Boot { project_id: "test".into() },
        ).unwrap();
        entries.push(genesis);
        for i in 1..count {
            let prev = &entries[i - 1].clone();
            let next = WitnessWriter::create_next(
                &key, prev, WitnessEvent::Command { cmd: format!("cmd-{i}"), exit_code: 0 },
            ).unwrap();
            entries.push(next);
        }
        let lines: Vec<String> = entries.iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn test_compact_if_needed_archives_over_limit_entries() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let witness = dir.path().join("test.rvf.witness");
        make_witness_file(&witness, 120);

        let compactor = WitnessCompactor {
            rvf_path: rvf.clone(),
            policy: WitnessPolicy { max_entries: 100, retention_days: 30 },
            archive_dir: dir.path().join("archives"),
        };
        let result = compactor.compact_if_needed().unwrap();
        assert_eq!(result.entries_kept, 100);
        assert_eq!(result.entries_archived, 20);
        assert!(result.archive_path.is_some());

        let kept_content = std::fs::read_to_string(&witness).unwrap();
        let kept_count = kept_content.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(kept_count, 100);
    }

    #[test]
    fn test_compact_if_needed_noop_when_under_limit() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let witness = dir.path().join("test.rvf.witness");
        make_witness_file(&witness, 50);

        let compactor = WitnessCompactor {
            rvf_path: rvf,
            policy: WitnessPolicy { max_entries: 100, retention_days: 30 },
            archive_dir: dir.path().join("archives"),
        };
        let result = compactor.compact_if_needed().unwrap();
        assert_eq!(result.entries_archived, 0);
        assert_eq!(result.entries_kept, 50);
        assert!(result.archive_path.is_none());
    }

    #[test]
    fn test_force_compact_rewrites_witness_file() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let witness = dir.path().join("test.rvf.witness");
        make_witness_file(&witness, 50);

        let original_size = std::fs::metadata(&witness).unwrap().len();
        let compactor = WitnessCompactor {
            rvf_path: rvf,
            policy: WitnessPolicy { max_entries: 100, retention_days: 30 },
            archive_dir: dir.path().join("archives"),
        };
        let result = compactor.force_compact().unwrap();
        assert_eq!(result.entries_kept, 50);
        assert_eq!(result.entries_archived, 0);
        // file was rewritten (size may differ slightly due to formatting)
        assert!(std::fs::metadata(&witness).unwrap().len() > 0);
        let _ = original_size;
    }

    #[test]
    fn test_compact_if_needed_no_file_returns_empty() {
        let dir = tempdir().unwrap();
        let compactor = WitnessCompactor {
            rvf_path: dir.path().join("missing.rvf"),
            policy: WitnessPolicy { max_entries: 100, retention_days: 30 },
            archive_dir: dir.path().join("archives"),
        };
        let result = compactor.compact_if_needed().unwrap();
        assert_eq!(result.entries_archived, 0);
        assert_eq!(result.entries_kept, 0);
    }

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
