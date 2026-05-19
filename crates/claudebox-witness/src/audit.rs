use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha3::{Digest, Sha3_256};

use crate::{WitnessEntry, WitnessEvent};

pub struct ChainVerifyResult {
    pub is_valid: bool,
    pub broken_at_seq: Option<u64>,
}

/// Verify the hash-linked chain of witness entries.
///
/// For each entry:
/// 1. Recompute SHA3-256(serde_json::to_vec(&entry.event)) and compare to `payload_hash`.
/// 2. For entries after genesis: verify `prev_hash == previous.payload_hash`.
/// 3. If `verifying_key` is `Some`, verify the Ed25519 signature over
///    `seq || ts_nanos || payload_hash || prev_hash` (matching `WitnessWriter::sign`).
///
/// Pass `None` for `verifying_key` when the key is not yet available (e.g. before
/// rvf-runtime integration); hash-link verification still runs.
pub fn verify_chain(entries: &[WitnessEntry], verifying_key: Option<&VerifyingKey>) -> ChainVerifyResult {
    for (i, entry) in entries.iter().enumerate() {
        // Step 1: recompute payload hash and compare.
        let event_bytes = match serde_json::to_vec(&entry.event) {
            Ok(b) => b,
            Err(_) => {
                return ChainVerifyResult { is_valid: false, broken_at_seq: Some(entry.seq) };
            }
        };
        let computed: [u8; 32] = {
            let mut hasher = Sha3_256::new();
            hasher.update(&event_bytes);
            hasher.finalize().into()
        };
        if computed != entry.payload_hash {
            return ChainVerifyResult { is_valid: false, broken_at_seq: Some(entry.seq) };
        }

        // Step 2: verify prev_hash links to previous entry's payload_hash.
        if i > 0 {
            let prev = &entries[i - 1];
            if entry.prev_hash != prev.payload_hash {
                return ChainVerifyResult { is_valid: false, broken_at_seq: Some(entry.seq) };
            }
        }

        // Step 3: verify Ed25519 signature when a key is provided.
        if let Some(vk) = verifying_key {
            let sig_bytes: [u8; 64] = match entry.signature.as_slice().try_into() {
                Ok(b) => b,
                Err(_) => {
                    return ChainVerifyResult { is_valid: false, broken_at_seq: Some(entry.seq) };
                }
            };
            let signature = Signature::from_bytes(&sig_bytes);

            // Reconstruct the signed message: seq || ts_nanos || payload_hash || prev_hash
            let mut msg = Vec::with_capacity(8 + 16 + 32 + 32);
            msg.extend_from_slice(&entry.seq.to_le_bytes());
            msg.extend_from_slice(&entry.ts_nanos.to_le_bytes());
            msg.extend_from_slice(&entry.payload_hash);
            msg.extend_from_slice(&entry.prev_hash);

            if vk.verify(&msg, &signature).is_err() {
                return ChainVerifyResult { is_valid: false, broken_at_seq: Some(entry.seq) };
            }
        }
    }

    ChainVerifyResult { is_valid: true, broken_at_seq: None }
}

/// Format witness entries as an ASCII table for human-readable audit output.
pub fn format_audit_table(entries: &[WitnessEntry]) -> String {
    let mut lines = Vec::new();

    let valid = verify_chain(entries, None);
    let chain_status = if valid.is_valid {
        "OK"
    } else {
        "BROKEN"
    };

    lines.push(format!(
        "Chain integrity: {} ({} entries)",
        chain_status,
        entries.len()
    ));
    lines.push(format!(
        "{:<6} {:<20} {:<18} {}",
        "SEQ", "TIMESTAMP_NS", "EVENT", "DETAILS"
    ));
    lines.push("-".repeat(80));

    for entry in entries {
        let event_type = event_type_label(&entry.event);
        let details = event_details(&entry.event);
        lines.push(format!(
            "{:<6} {:<20} {:<18} {}",
            entry.seq, entry.ts_nanos, event_type, details
        ));
    }

    lines.join("\n")
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Format witness entries as JSON for machine-readable audit output.
///
/// Returns an error if serialization fails, so callers can detect and surface
/// data loss rather than silently receiving empty JSON.
pub fn format_audit_json(entries: &[WitnessEntry]) -> anyhow::Result<String> {
    let valid = verify_chain(entries, None);
    let payload = serde_json::json!({
        "chain_valid": valid.is_valid,
        "broken_at_seq": valid.broken_at_seq,
        "entries": entries.iter().map(|e| serde_json::json!({
            "seq": e.seq,
            "ts_nanos": e.ts_nanos,
            "event": e.event,
            "payload_hash": to_hex(&e.payload_hash),
            "prev_hash": to_hex(&e.prev_hash),
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&payload)
        .map_err(|e| anyhow::anyhow!("failed to serialize audit JSON: {e}"))
}

fn event_type_label(event: &WitnessEvent) -> &'static str {
    match event {
        WitnessEvent::Boot { .. } => "BOOT",
        WitnessEvent::Shutdown { .. } => "SHUTDOWN",
        WitnessEvent::Command { .. } => "COMMAND",
        WitnessEvent::FileWrite { .. } => "FILE_WRITE",
        WitnessEvent::FileDelete { .. } => "FILE_DELETE",
        WitnessEvent::NetworkRequest { .. } => "NETWORK_REQUEST",
        WitnessEvent::PackageInstall { .. } => "PACKAGE_INSTALL",
        WitnessEvent::Snapshot { .. } => "SNAPSHOT",
        WitnessEvent::Branch { .. } => "BRANCH",
        WitnessEvent::Rollback { .. } => "ROLLBACK",
        WitnessEvent::KernelUpgrade { .. } => "KERNEL_UPGRADE",
        WitnessEvent::WitnessCompact { .. } => "WITNESS_COMPACT",
        WitnessEvent::VecReconcile { .. } => "VEC_RECONCILE",
        WitnessEvent::FormatMigrate { .. } => "FORMAT_MIGRATE",
    }
}

fn event_details(event: &WitnessEvent) -> String {
    match event {
        WitnessEvent::Boot { project_id } => format!("project_id={}", project_id),
        WitnessEvent::Shutdown { reason } => format!("reason={}", reason),
        WitnessEvent::Command { cmd, exit_code } => format!("cmd={:?} exit={}", cmd, exit_code),
        WitnessEvent::FileWrite { path, size_bytes } => {
            format!("path={:?} size={}", path, size_bytes)
        }
        WitnessEvent::FileDelete { path } => format!("path={:?}", path),
        WitnessEvent::NetworkRequest { domain, method, status } => {
            format!("{}  {}  {}", method, domain, status)
        }
        WitnessEvent::PackageInstall { name, version, registry } => {
            format!("{}@{} from {}", name, version, registry)
        }
        WitnessEvent::Snapshot { name } => format!("name={}", name),
        WitnessEvent::Branch { child_name } => format!("child={}", child_name),
        WitnessEvent::Rollback { target } => format!("target={}", target),
        WitnessEvent::KernelUpgrade { from_hash, to_hash } => {
            format!("{}..{}", &from_hash[..8.min(from_hash.len())], &to_hash[..8.min(to_hash.len())])
        }
        WitnessEvent::WitnessCompact { entries_archived, archive_path } => {
            format!("{} entries → {}", entries_archived, archive_path)
        }
        WitnessEvent::VecReconcile { files_removed } => format!("{} files removed", files_removed),
        WitnessEvent::FormatMigrate { from_version, to_version } => {
            format!("v{} → v{}", from_version, to_version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::WitnessWriter;

    #[test]
    fn test_audit_table_output_format() {
        let entries = vec![WitnessEntry {
            seq: 0,
            ts_nanos: 0,
            event: WitnessEvent::Boot { project_id: "x".into() },
            payload_hash: {
                // Compute correct hash so the table can verify the chain
                let event_bytes =
                    serde_json::to_vec(&WitnessEvent::Boot { project_id: "x".into() }).unwrap();
                let mut hasher = Sha3_256::new();
                hasher.update(&event_bytes);
                hasher.finalize().into()
            },
            prev_hash: [0u8; 32],
            signature: vec![0u8; 64],
        }];
        let output = format_audit_table(&entries);
        assert!(output.contains("Chain integrity"));
        assert!(output.contains("BOOT"));
    }

    #[test]
    fn test_chain_integrity_valid_hash_only() {
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let genesis = WitnessWriter::create_genesis(
            &signing_key,
            WitnessEvent::Boot { project_id: "test".into() },
        )
        .unwrap();
        let result = verify_chain(&[genesis], None);
        assert!(result.is_valid);
        assert_eq!(result.broken_at_seq, None);
    }

    #[test]
    fn test_chain_integrity_valid_with_signature() {
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        let genesis = WitnessWriter::create_genesis(
            &signing_key,
            WitnessEvent::Boot { project_id: "test".into() },
        )
        .unwrap();
        let next = WitnessWriter::create_next(
            &signing_key,
            &genesis,
            WitnessEvent::Command { cmd: "cargo test".into(), exit_code: 0 },
        )
        .unwrap();
        let result = verify_chain(&[genesis, next], Some(&verifying_key));
        assert!(result.is_valid);
        assert_eq!(result.broken_at_seq, None);
    }

    #[test]
    fn test_chain_integrity_detects_tampered_signature() {
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        let mut genesis = WitnessWriter::create_genesis(
            &signing_key,
            WitnessEvent::Boot { project_id: "test".into() },
        )
        .unwrap();
        // Tamper with the signature bytes
        if let Some(b) = genesis.signature.first_mut() { *b ^= 0xFF; }
        let result = verify_chain(&[genesis], Some(&verifying_key));
        assert!(!result.is_valid);
        assert_eq!(result.broken_at_seq, Some(0));
    }

    #[test]
    fn test_chain_integrity_detects_tamper() {
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let mut genesis = WitnessWriter::create_genesis(
            &signing_key,
            WitnessEvent::Boot { project_id: "test".into() },
        )
        .unwrap();
        // Tamper with payload_hash
        genesis.payload_hash[0] ^= 0xFF;
        let result = verify_chain(&[genesis], None);
        assert!(!result.is_valid);
    }
}
