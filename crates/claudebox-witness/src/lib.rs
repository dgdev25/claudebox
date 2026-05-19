pub mod audit;
pub mod compaction;
pub mod writer;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Read a JSONL witness file into a `Vec<WitnessEntry>`.
///
/// Returns an empty `Vec` if the file does not exist. Blank lines are skipped.
/// Any malformed line aborts the read with an error — partial recovery is
/// intentionally not supported because a broken line breaks the hash chain.
pub fn read_jsonl_entries(path: &Path) -> anyhow::Result<Vec<WitnessEntry>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(path)
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

/// A 32-byte hash stored as hex for JSON compatibility.
pub type Hash32 = [u8; 32];
/// A 64-byte Ed25519 signature stored as a byte sequence.
/// Using Vec<u8> for serde compatibility; length invariant enforced by constructors.
pub type Sig64 = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessEntry {
    pub seq: u64,
    pub ts_nanos: u128,
    pub event: WitnessEvent,
    /// SHA3-256 of event payload (32 bytes, serialised as byte sequence).
    #[serde(with = "serde_bytes_array")]
    pub payload_hash: Hash32,
    /// Hash of previous entry; genesis = [0u8; 32].
    #[serde(with = "serde_bytes_array")]
    pub prev_hash: Hash32,
    /// Ed25519 signature over (seq + ts + payload_hash + prev_hash).
    pub signature: Sig64,
}

/// Custom serde helper for fixed-size byte arrays.
mod serde_bytes_array {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        use serde::de::Error;
        let v: Vec<u8> = serde::de::Deserialize::deserialize(d)?;
        v.try_into().map_err(|_| D::Error::custom("expected exactly 32 bytes"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WitnessEvent {
    Boot            { project_id: String },
    Shutdown        { reason: String },
    Command         { cmd: String, exit_code: i32 },
    FileWrite       { path: String, size_bytes: u64 },
    FileDelete      { path: String },
    NetworkRequest  { domain: String, method: String, status: u16 },
    PackageInstall  { name: String, version: String, registry: String },
    Snapshot        { name: String },
    Branch          { child_name: String },
    Rollback        { target: String },
    /// REMEDIATION BS-2: kernel upgrade tracking.
    KernelUpgrade   { from_hash: String, to_hash: String },
    /// REMEDIATION BS-1: witness compaction tracking.
    WitnessCompact  { entries_archived: u32, archive_path: String },
    /// REMEDIATION BS-5: stale VEC entry tracking.
    VecReconcile    { files_removed: u32 },
    /// REMEDIATION BS-7: format migration tracking.
    FormatMigrate   { from_version: u8, to_version: u8 },
    /// Allowlist domain update.
    AllowlistUpdate { added: Vec<String>, removed: Vec<String> },
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_event_all_variants_serialise() {
        let events = vec![
            WitnessEvent::Boot { project_id: "x".into() },
            WitnessEvent::Shutdown { reason: "graceful".into() },
            WitnessEvent::KernelUpgrade { from_hash: "a".into(), to_hash: "b".into() },
            WitnessEvent::WitnessCompact { entries_archived: 100, archive_path: "/tmp/x".into() },
            WitnessEvent::VecReconcile { files_removed: 3 },
            WitnessEvent::FormatMigrate { from_version: 1, to_version: 2 },
        ];
        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            assert!(!json.is_empty());
        }
    }
}
