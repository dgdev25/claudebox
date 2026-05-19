use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

/// Convert Unix epoch seconds to `(year, month)`.
///
/// Uses Howard Hinnant's civil-calendar algorithm — no external deps.
pub fn epoch_secs_to_ym(secs: u64) -> (i32, u32) {
    let days = (secs / 86400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z / 146_097 } else { (z - 146_096) / 146_097 };
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32)
}

/// List archive files in `archive_dir` whose epoch timestamp falls in the given `YYYY-MM` month.
///
/// Returns an empty `Vec` when the directory does not exist.
/// Files must be named `witness-archive-{epoch_secs}.jsonl`.
pub fn find_archives_for_month(archive_dir: &Path, month: &str) -> anyhow::Result<Vec<PathBuf>> {
    if !archive_dir.exists() {
        return Ok(vec![]);
    }
    let (target_y, target_m) = parse_ym(month)
        .ok_or_else(|| anyhow::anyhow!("invalid month format '{month}' — expected YYYY-MM"))?;

    let read_dir = std::fs::read_dir(archive_dir)
        .map_err(|e| anyhow::anyhow!("failed to read archive dir: {e}"))?;

    // Collect (epoch_secs, path) so we can sort numerically rather than
    // lexicographically — robust if epoch widths ever differ.
    let mut matches: Vec<(u64, PathBuf)> = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|e| anyhow::anyhow!("directory read error: {e}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(epoch_str) = name
            .strip_prefix("witness-archive-")
            .and_then(|s| s.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let Ok(epoch_secs) = epoch_str.parse::<u64>() else {
            continue;
        };
        let (y, m) = epoch_secs_to_ym(epoch_secs);
        if y == target_y && m == target_m {
            matches.push((epoch_secs, entry.path()));
        }
    }
    matches.sort_by_key(|(epoch, _)| *epoch);
    Ok(matches.into_iter().map(|(_, p)| p).collect())
}

fn parse_ym(s: &str) -> Option<(i32, u32)> {
    let (y, m) = s.split_once('-')?;
    let year = y.parse::<i32>().ok()?;
    let month = m.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
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
        crate::read_jsonl_entries(&self.witness_path())
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

        // Append a trailing blank line so we can prove the file was rewritten
        // (the rewrite path normalises blank lines away).
        let original = std::fs::read_to_string(&witness).unwrap();
        std::fs::write(&witness, format!("{original}\n\n")).unwrap();
        let original_len = std::fs::metadata(&witness).unwrap().len();

        let compactor = WitnessCompactor {
            rvf_path: rvf,
            policy: WitnessPolicy { max_entries: 100, retention_days: 30 },
            archive_dir: dir.path().join("archives"),
        };
        let result = compactor.force_compact().unwrap();
        assert_eq!(result.entries_kept, 50);
        assert_eq!(result.entries_archived, 0);

        let new_len = std::fs::metadata(&witness).unwrap().len();
        assert!(new_len < original_len, "force_compact should normalise the file");

        // And the rewritten content is still parseable as 50 entries.
        let kept = crate::read_jsonl_entries(&witness).unwrap();
        assert_eq!(kept.len(), 50);
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
    fn test_epoch_secs_to_ym_unix_epoch() {
        assert_eq!(epoch_secs_to_ym(0), (1970, 1));
    }

    #[test]
    fn test_epoch_secs_to_ym_known_dates() {
        // 2026-01-01T00:00:00Z = 1767225600
        assert_eq!(epoch_secs_to_ym(1_767_225_600), (2026, 1));
        // 2026-02-01T00:00:00Z = 1769904000
        assert_eq!(epoch_secs_to_ym(1_769_904_000), (2026, 2));
        // 2026-01-15T00:00:00Z = 1768435200
        assert_eq!(epoch_secs_to_ym(1_768_435_200), (2026, 1));
    }

    #[test]
    fn test_find_archives_for_month_returns_matching_files() {
        let dir = tempdir().unwrap();
        // 2026-01-01: 1767225600
        std::fs::write(dir.path().join("witness-archive-1767225600.jsonl"), "").unwrap();
        // 2026-01-15: 1768435200
        std::fs::write(dir.path().join("witness-archive-1768435200.jsonl"), "").unwrap();
        // 2026-02-01: 1769904000
        std::fs::write(dir.path().join("witness-archive-1769904000.jsonl"), "").unwrap();
        // not a witness archive
        std::fs::write(dir.path().join("other.txt"), "").unwrap();

        let jan = find_archives_for_month(dir.path(), "2026-01").unwrap();
        assert_eq!(jan.len(), 2);

        let feb = find_archives_for_month(dir.path(), "2026-02").unwrap();
        assert_eq!(feb.len(), 1);

        let mar = find_archives_for_month(dir.path(), "2026-03").unwrap();
        assert!(mar.is_empty());
    }

    #[test]
    fn test_find_archives_missing_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let result = find_archives_for_month(&dir.path().join("nonexistent"), "2026-01").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_archives_invalid_month_returns_error() {
        let dir = tempdir().unwrap();
        let err = find_archives_for_month(dir.path(), "not-a-month");
        assert!(err.is_err());
    }

}
