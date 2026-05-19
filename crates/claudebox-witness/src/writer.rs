use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use ed25519_dalek::{Signer, SigningKey};
use sha3::{Digest, Sha3_256};

use crate::{WitnessEntry, WitnessEvent};

/// Constructs signed `WitnessEntry` values forming a hash-linked chain.
pub struct WitnessWriter;

impl WitnessWriter {
    /// Create the first entry in a new witness chain.
    ///
    /// * `seq`       = 0
    /// * `prev_hash` = `[0u8; 32]`
    /// * `payload_hash` = SHA3-256(JSON-serialised `event`)
    /// * `signature` = Ed25519 over `(seq || ts_nanos || payload_hash || prev_hash)`
    pub fn create_genesis(
        signing_key: &SigningKey,
        event: WitnessEvent,
    ) -> anyhow::Result<WitnessEntry> {
        let prev_hash = [0u8; 32];
        Self::build_entry(signing_key, 0, prev_hash, event)
    }

    /// Create the next entry, chaining from `prev`.
    ///
    /// * `seq`       = `prev.seq + 1`
    /// * `prev_hash` = `prev.payload_hash`
    pub fn create_next(
        signing_key: &SigningKey,
        prev: &WitnessEntry,
        event: WitnessEvent,
    ) -> anyhow::Result<WitnessEntry> {
        Self::build_entry(signing_key, prev.seq + 1, prev.payload_hash, event)
    }

    // ── private helpers ──────────────────────────────────────────────────────

    fn build_entry(
        signing_key: &SigningKey,
        seq: u64,
        prev_hash: [u8; 32],
        event: WitnessEvent,
    ) -> anyhow::Result<WitnessEntry> {
        let payload_bytes =
            serde_json::to_vec(&event).context("failed to serialise WitnessEvent")?;

        let payload_hash: [u8; 32] = {
            let mut hasher = Sha3_256::new();
            hasher.update(&payload_bytes);
            hasher.finalize().into()
        };

        let ts_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX epoch")?
            .as_nanos();

        let signature = Self::sign(signing_key, seq, ts_nanos, &payload_hash, &prev_hash);

        Ok(WitnessEntry {
            seq,
            ts_nanos,
            event,
            payload_hash,
            prev_hash,
            signature,
        })
    }

    /// Sign `seq || ts_nanos || payload_hash || prev_hash` with Ed25519.
    ///
    /// Returns the 64-byte signature as `Vec<u8>`.
    fn sign(
        signing_key: &SigningKey,
        seq: u64,
        ts_nanos: u128,
        payload_hash: &[u8; 32],
        prev_hash: &[u8; 32],
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(8 + 16 + 32 + 32);
        msg.extend_from_slice(&seq.to_le_bytes());
        msg.extend_from_slice(&ts_nanos.to_le_bytes());
        msg.extend_from_slice(payload_hash);
        msg.extend_from_slice(prev_hash);

        signing_key.sign(&msg).to_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;

    #[test]
    fn test_witness_chain_genesis_has_zero_prev_hash() {
        let signing_key = SigningKey::generate(&mut thread_rng());
        let entry = WitnessWriter::create_genesis(
            &signing_key,
            WitnessEvent::Boot { project_id: "test".into() },
        )
        .unwrap();
        assert_eq!(entry.seq, 0);
        assert_eq!(entry.prev_hash, [0u8; 32]);
    }

    #[test]
    fn test_witness_chain_prev_hash_links_correctly() {
        let signing_key = SigningKey::generate(&mut thread_rng());
        let genesis = WitnessWriter::create_genesis(
            &signing_key,
            WitnessEvent::Boot { project_id: "test".into() },
        )
        .unwrap();
        let next = WitnessWriter::create_next(
            &signing_key,
            &genesis,
            WitnessEvent::Command { cmd: "cargo build".into(), exit_code: 0 },
        )
        .unwrap();
        assert_eq!(next.seq, 1);
        assert_eq!(next.prev_hash, genesis.payload_hash);
    }
}
