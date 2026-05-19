use std::path::{Path, PathBuf};

use claudebox_witness::writer::WitnessWriter;
use claudebox_witness::{WitnessEntry, WitnessEvent};
use ed25519_dalek::SigningKey;

/// Path of the witness JSONL sidecar for a given `.rvf` file.
///
/// `foo/appliance.rvf` → `foo/appliance.rvf.witness`
pub fn witness_path_for_rvf(rvf_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.witness", rvf_path.display()))
}

/// Path of the persisted Ed25519 seed for a given `.rvf` file.
///
/// `foo/appliance.rvf` → `foo/appliance.rvf.key`
pub fn signing_key_path_for_rvf(rvf_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.key", rvf_path.display()))
}

/// Path of the live manifest JSON sidecar for a given `.rvf` file.
///
/// The `.rvf` embeds the genesis manifest; this sidecar holds updates.
/// `foo/appliance.rvf` → `foo/appliance.rvf.manifest.json`
pub fn manifest_sidecar_path(rvf_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.manifest.json", rvf_path.display()))
}

/// Directory that holds monthly JSONL archives produced by `WitnessCompactor`.
///
/// `foo/appliance.rvf` → `foo/appliance.rvf.witness-archives/`
pub fn witness_archive_dir(rvf_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.witness-archives", rvf_path.display()))
}

/// Path of the META session-state JSON sidecar for a given `.rvf` file.
///
/// rvf-runtime exposes no public META_SEG reader, so per-session state
/// (task context, history, scratchpad) is persisted as a JSON sidecar
/// alongside the `.rvf` instead.
///
/// `foo/appliance.rvf` → `foo/appliance.rvf.meta.json`
pub fn meta_sidecar_path(rvf_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta.json", rvf_path.display()))
}

/// Load all `WitnessEntry` records from the `.witness` JSONL sidecar.
///
/// Returns an empty `Vec` when the file does not exist.
pub fn load_witness_entries(rvf_path: &Path) -> anyhow::Result<Vec<WitnessEntry>> {
    claudebox_witness::read_jsonl_entries(&witness_path_for_rvf(rvf_path))
}

/// Append a signed `WitnessEntry` to the `.witness` JSONL sidecar.
///
/// Reads the Ed25519 seed from `<rvf>.key` to sign the entry.
/// If no key file exists the entry is written with an all-zero signature
/// (hash-chain integrity still holds; only non-repudiation is lost).
/// The new entry chains from the last entry in the file.
pub fn append_witness_entry(rvf_path: &Path, event: WitnessEvent) -> anyhow::Result<()> {
    let signing_key = load_or_ephemeral_key(rvf_path);
    let entries = load_witness_entries(rvf_path)?;

    let new_entry = if let Some(prev) = entries.last() {
        WitnessWriter::create_next(&signing_key, prev, event)
    } else {
        WitnessWriter::create_genesis(&signing_key, event)
    }
    .map_err(|e| anyhow::anyhow!("witness entry creation failed: {e}"))?;

    let json = serde_json::to_string(&new_entry)
        .map_err(|e| anyhow::anyhow!("witness serialisation failed: {e}"))?;

    let witness_path = witness_path_for_rvf(rvf_path);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&witness_path)
        .map_err(|e| anyhow::anyhow!("failed to open witness file: {e}"))?;

    use std::io::Write;
    writeln!(file, "{json}").map_err(|e| anyhow::anyhow!("failed to write witness entry: {e}"))
}

/// Load the persisted signing key for an `.rvf`, or generate an ephemeral key.
///
/// If the key file exists but is the wrong size (corrupt), we log a warning
/// instead of silently using an ephemeral key — using an ephemeral key would
/// produce a chain whose signatures no auditor can verify against the
/// previously published verifying key.
fn load_or_ephemeral_key(rvf_path: &Path) -> SigningKey {
    let key_path = signing_key_path_for_rvf(rvf_path);
    match std::fs::read(&key_path) {
        Ok(bytes) => match <[u8; 32]>::try_from(bytes.as_slice()) {
            Ok(arr) => SigningKey::from_bytes(&arr),
            Err(_) => {
                tracing::warn!(
                    path = %key_path.display(),
                    len = bytes.len(),
                    "signing key file has wrong length (expected 32 bytes); falling back to ephemeral key — chain non-repudiation lost"
                );
                SigningKey::generate(&mut rand::rngs::OsRng)
            }
        },
        Err(_) => SigningKey::generate(&mut rand::rngs::OsRng),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_test_rvf(dir: &std::path::Path) -> PathBuf {
        use rvf_runtime::{options::RvfOptions, RvfStore};
        let rvf = dir.join("test.rvf");
        let opts = RvfOptions { dimension: 1, ..Default::default() };
        let mut store = RvfStore::create(&rvf, opts).unwrap();
        store.embed_kernel(0x00, 0x01, 0, &[], 2222, Some("{\"version\":1,\"project_id\":\"test-id\",\"project_name\":\"test\",\"language\":{\"Single\":{\"lang\":\"Node\",\"version\":\"22\"}},\"created_at\":\"\",\"kernel_built_at\":\"\",\"network\":{\"allow_domains\":[\"registry.npmjs.org\"],\"allow_localhost\":true,\"dns_server\":\"1.1.1.1\"},\"resources\":{\"memory_mb\":512,\"vcpus\":1,\"disk_gb\":8,\"network_mbps\":100},\"kernel\":{\"arch\":\"x86_64\",\"ssh_port\":2222,\"mcp_port\":7878},\"witness\":{\"max_entries\":10000,\"retention_days\":30}}")).unwrap();
        store.close().unwrap();
        rvf
    }

    #[test]
    fn test_witness_archive_dir_appends_archive_suffix() {
        let p = Path::new("/tmp/foo.rvf");
        assert_eq!(witness_archive_dir(p), PathBuf::from("/tmp/foo.rvf.witness-archives"));
    }

    #[test]
    fn test_witness_path_appends_witness_suffix() {
        let p = Path::new("/tmp/foo.rvf");
        assert_eq!(witness_path_for_rvf(p), PathBuf::from("/tmp/foo.rvf.witness"));
    }

    #[test]
    fn test_signing_key_path_appends_key_suffix() {
        let p = Path::new("/tmp/foo.rvf");
        assert_eq!(signing_key_path_for_rvf(p), PathBuf::from("/tmp/foo.rvf.key"));
    }

    #[test]
    fn test_manifest_sidecar_path_appends_manifest_suffix() {
        let p = Path::new("/tmp/foo.rvf");
        assert_eq!(manifest_sidecar_path(p), PathBuf::from("/tmp/foo.rvf.manifest.json"));
    }

    #[test]
    fn test_append_witness_entry_creates_file_and_chains() {
        let dir = tempdir().unwrap();
        let rvf = make_test_rvf(dir.path());

        append_witness_entry(&rvf, WitnessEvent::Boot { project_id: "test".into() }).unwrap();
        let entries = load_witness_entries(&rvf).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].seq, 0);
        assert_eq!(entries[0].prev_hash, [0u8; 32]);

        append_witness_entry(&rvf, WitnessEvent::Shutdown { reason: "graceful".into() }).unwrap();
        let entries = load_witness_entries(&rvf).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].seq, 1);
        assert_eq!(entries[1].prev_hash, entries[0].payload_hash);
    }

    #[test]
    fn test_load_witness_entries_empty_when_no_file() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("nonexistent.rvf");
        let entries = load_witness_entries(&rvf).unwrap();
        assert!(entries.is_empty());
    }
}
