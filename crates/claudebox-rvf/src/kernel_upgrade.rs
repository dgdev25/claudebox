use std::path::{Path, PathBuf};

use sha3::{Digest, Sha3_256};

/// The RVF segment name that contains the compiled kernel image.
#[allow(dead_code)] // will be used in Phase 5 segment surgery
const KERNEL_SEG: &str = "KERNEL_SEG";

/// Performs in-place kernel upgrades on an `.rvf` appliance file.
///
/// The upgrade operation preserves all non-kernel segments (META, VEC,
/// WITNESS, MANIFEST) and replaces only `KERNEL_SEG` with the new image.
/// A `WitnessEvent::KernelUpgrade` entry is appended to record the
/// transition cryptographically.
pub struct KernelUpgrader {
    pub rvf_path: PathBuf,
}

/// Result of a successful kernel upgrade, carrying the before/after hashes.
pub struct UpgradeResult {
    pub from_hash: String,
    pub to_hash: String,
}

impl KernelUpgrader {
    /// Computes the SHA3-256 hex digest of the KERNEL_SEG bytes inside the
    /// `.rvf` file.
    ///
    /// TODO(Phase-5): wire in rvf-runtime to extract KERNEL_SEG bytes.
    /// For now this reads the entire file as a proxy for the kernel bytes.
    pub fn hash_current_kernel(&self) -> anyhow::Result<String> {
        let bytes = std::fs::read(&self.rvf_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to read rvf file at {}: {}",
                self.rvf_path.display(),
                e
            )
        })?;
        Ok(hex_sha3_256(&bytes))
    }

    /// Returns all segments from the `.rvf` file except `KERNEL_SEG`, as
    /// `(segment_name, bytes)` pairs.
    ///
    /// TODO(Phase-5): replace with rvf-runtime segment reader.
    pub fn extract_non_kernel_segments(&self) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
        // Stub: rvf-runtime segment extraction is deferred to Phase 5.
        // Returns an empty vec until the rvf-runtime API is wired in.
        anyhow::bail!(
            "extract_non_kernel_segments: rvf-runtime segment reader \
             not yet integrated — deferred to Phase 5"
        )
    }

    /// Upgrades the kernel inside the `.rvf` appliance at `self.rvf_path`.
    ///
    /// Steps:
    /// 1. Hash the current KERNEL_SEG (`from_hash`).
    /// 2. Extract all non-kernel segments.
    /// 3. Build an upgraded `.rvf` using [`InitTransaction`] for atomicity.
    /// 4. Append a `WitnessEvent::KernelUpgrade` entry.
    /// 5. Update `kernel_built_at` in MANIFEST_SEG.
    ///
    /// TODO(Phase-5): implement full segment surgery once rvf-runtime
    /// read/write API is available.
    pub async fn upgrade(&self, new_kernel_path: &Path) -> anyhow::Result<UpgradeResult> {
        // Compute from_hash (uses the stub hash above).
        let from_hash = self.hash_current_kernel()?;

        // Compute to_hash from the incoming kernel file.
        let new_kernel_bytes = tokio::fs::read(new_kernel_path).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to read new kernel at {}: {}",
                new_kernel_path.display(),
                e
            )
        })?;
        let to_hash = hex_sha3_256(&new_kernel_bytes);

        // TODO(Phase-5): use extract_non_kernel_segments() + InitTransaction
        // to atomically rebuild the .rvf with the new KERNEL_SEG, updated
        // kernel_built_at in MANIFEST_SEG, and a KernelUpgrade WitnessEvent.
        anyhow::bail!(
            "upgrade(): full segment surgery not yet implemented — \
             requires rvf-runtime read/write API (deferred to Phase 5). \
             from_hash={from_hash}, to_hash={to_hash}"
        )
    }
}

/// Returns the SHA3-256 digest of `data` as a lowercase hex string.
fn hex_sha3_256(data: &[u8]) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upgrade_result_has_hashes() {
        let r = UpgradeResult {
            from_hash: "abc".into(),
            to_hash: "def".into(),
        };
        assert_ne!(r.from_hash, r.to_hash);
    }

    #[test]
    #[ignore = "requires rvf-cli, docker"]
    fn test_upgrade_kernel_preserves_meta_seg() {
        // claudebox init with session data in META_SEG
        // KernelUpgrader::upgrade()
        // Verify META_SEG identical to pre-upgrade
        // Verify WITNESS_SEG has KernelUpgrade event
        // Verify kernel_built_at updated
        // Full integration test — deferred to Phase 10
    }
}
