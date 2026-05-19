use std::path::Path;

use claudebox_core::manifest::ClaudeBoxManifest;
use claudebox_core::witness::{signing_key_path_for_rvf, witness_path_for_rvf};
use claudebox_witness::writer::WitnessWriter;
use claudebox_witness::WitnessEvent;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rvf_runtime::options::RvfOptions;
use rvf_runtime::RvfStore;
use rvf_types::kernel::KernelArch;

/// Map a manifest arch string ("x86_64", "aarch64") to the `KernelArch` byte
/// used by `rvf-types`. Returns an error for unknown strings.
pub fn arch_str_to_kernel_arch(arch: &str) -> anyhow::Result<u8> {
    match arch {
        "x86_64" => Ok(KernelArch::X86_64 as u8),
        "aarch64" => Ok(KernelArch::Aarch64 as u8),
        "riscv64" => Ok(KernelArch::Riscv64 as u8),
        other => anyhow::bail!("unsupported kernel arch '{other}'"),
    }
}

/// Builds a ClaudeBox RVF appliance from a manifest using `RvfStore`.
///
/// Each appliance has its own unique Ed25519 signing key, retained for
/// future CRYPTO-binding work. Persistence format is now the standard
/// RvfStore wire format (replaces the hand-rolled CLBX segments).
pub struct ApplianceBuilder {
    pub manifest: ClaudeBoxManifest,
    pub signing_key: SigningKey,
}

impl ApplianceBuilder {
    /// Create a new builder. Generates a fresh Ed25519 signing key.
    pub fn new(manifest: ClaudeBoxManifest) -> anyhow::Result<Self> {
        let signing_key = SigningKey::generate(&mut OsRng);
        Ok(ApplianceBuilder {
            manifest,
            signing_key,
        })
    }

    /// Write the appliance skeleton to `output_path` using `RvfStore`.
    ///
    /// The manifest JSON is stored as the kernel cmdline so it can be
    /// retrieved later via `store.extract_kernel()`.
    ///
    /// When `kernel_path` is `Some`, the bzImage bytes are embedded via
    /// `store.embed_kernel()`. When `None`, a zero-byte placeholder is
    /// embedded so the cmdline (manifest JSON) is still reachable.
    pub fn build_skeleton(
        &self,
        output_path: &Path,
        kernel_path: Option<&Path>,
    ) -> anyhow::Result<()> {
        let manifest_json = serde_json::to_string(&self.manifest)
            .map_err(|e| anyhow::anyhow!("failed to serialize manifest: {e}"))?;

        let options = RvfOptions {
            dimension: 1,
            ..Default::default()
        };

        let mut store = RvfStore::create(output_path, options)
            .map_err(|e| anyhow::anyhow!("RvfStore::create failed: {e:?}"))?;

        let kernel_bytes: Vec<u8> = if let Some(kp) = kernel_path {
            std::fs::read(kp).map_err(|e| {
                anyhow::anyhow!("failed to read kernel from {}: {e}", kp.display())
            })?
        } else {
            vec![]
        };

        let arch_byte = arch_str_to_kernel_arch(&self.manifest.kernel.arch)?;

        store
            .embed_kernel(
                arch_byte,
                0x01,
                0,
                &kernel_bytes,
                self.manifest.kernel.ssh_port,
                Some(&manifest_json),
            )
            .map_err(|e| anyhow::anyhow!("embed_kernel failed: {e:?}"))?;

        store
            .close()
            .map_err(|e| anyhow::anyhow!("RvfStore::close failed: {e:?}"))?;

        Ok(())
    }

    /// Append an eBPF segment — deferred to Phase 5 (eBPF pipeline).
    pub fn embed_ebpf(&self, _rvf_path: &Path, _ebpf_path: &Path) -> anyhow::Result<()> {
        anyhow::bail!("embed_ebpf not yet implemented — deferred to Phase 5 (eBPF pipeline)")
    }

    /// Write the genesis witness entry for a newly created appliance.
    ///
    /// Creates `<rvf_path>.witness` (JSONL) with a single signed `Boot` entry
    /// and persists the Ed25519 seed to `<rvf_path>.key` so future sessions
    /// can append chained entries. If the witness file already exists the call
    /// is a no-op (idempotent).
    pub fn write_genesis_witness(&self, rvf_path: &Path) -> anyhow::Result<()> {
        let witness_path = witness_path_for_rvf(rvf_path);
        if witness_path.exists() {
            return Ok(());
        }

        let entry = WitnessWriter::create_genesis(
            &self.signing_key,
            WitnessEvent::Boot { project_id: self.manifest.project_id.clone() },
        )
        .map_err(|e| anyhow::anyhow!("genesis witness failed: {e}"))?;

        let json = serde_json::to_string(&entry)
            .map_err(|e| anyhow::anyhow!("witness serialisation failed: {e}"))?;

        std::fs::write(&witness_path, format!("{json}\n"))
            .map_err(|e| anyhow::anyhow!("failed to write witness file: {e}"))?;

        let key_path = signing_key_path_for_rvf(rvf_path);
        std::fs::write(&key_path, self.signing_key.to_bytes())
            .map_err(|e| anyhow::anyhow!("failed to write signing key: {e}"))?;

        Ok(())
    }

    /// Verify the appliance at `rvf_path` via RvfStore (stub).
    ///
    /// The old CLBX CRYPTO-segment verification is replaced by RvfStore's
    /// built-in checksums. Full Ed25519 binding verification is deferred
    /// to Phase 5 when `embed_kernel_with_binding` is wired.
    pub fn verify(&self, rvf_path: &Path) -> anyhow::Result<()> {
        let store = RvfStore::open_readonly(rvf_path)
            .map_err(|e| anyhow::anyhow!("RvfStore::open_readonly failed: {e:?}"))?;
        let result = store
            .extract_kernel()
            .map_err(|e| anyhow::anyhow!("extract_kernel failed: {e:?}"))?;
        if result.is_none() {
            anyhow::bail!("no KERNEL_SEG found in {}", rvf_path.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claudebox_core::manifest::{
        KernelConfig, Lang, LanguageProfile, NetworkPolicy, ResourceLimits, SingleProfile,
        WitnessPolicy,
    };
    use tempfile::tempdir;
    use rvf_runtime::RvfStore;

    fn test_manifest() -> ClaudeBoxManifest {
        ClaudeBoxManifest {
            version: 1,
            project_id: "test-id".into(),
            project_name: "testapp".into(),
            language: LanguageProfile::Single(SingleProfile {
                lang: Lang::Node,
                version: "22".into(),
            }),
            created_at: "2026-05-19T00:00:00Z".into(),
            kernel_built_at: "2026-05-19T00:00:00Z".into(),
            network: NetworkPolicy {
                allow_domains: vec!["registry.npmjs.org".into()],
                allow_localhost: true,
                dns_server: "1.1.1.1".into(),
            },
            resources: ResourceLimits::default(),
            kernel: KernelConfig {
                arch: "x86_64".into(),
                ssh_port: 2222,
                mcp_port: 7878,
            },
            witness: WitnessPolicy::default(),
        }
    }

    #[test]
    fn test_write_genesis_witness_creates_witness_file() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&rvf, None).unwrap();
        builder.write_genesis_witness(&rvf).unwrap();
        let witness_path = witness_path_for_rvf(&rvf);
        assert!(witness_path.exists(), "witness file should be created");
    }

    #[test]
    fn test_write_genesis_witness_entry_is_valid_json() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&rvf, None).unwrap();
        builder.write_genesis_witness(&rvf).unwrap();
        let content = std::fs::read_to_string(witness_path_for_rvf(&rvf)).unwrap();
        let _entry: claudebox_witness::WitnessEntry = serde_json::from_str(content.trim()).unwrap();
    }

    #[test]
    fn test_write_genesis_witness_idempotent() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&rvf, None).unwrap();
        builder.write_genesis_witness(&rvf).unwrap();
        let first = std::fs::read_to_string(witness_path_for_rvf(&rvf)).unwrap();
        builder.write_genesis_witness(&rvf).unwrap(); // second call is noop
        let second = std::fs::read_to_string(witness_path_for_rvf(&rvf)).unwrap();
        assert_eq!(first, second, "second write should be a noop");
    }

    #[test]
    fn test_arch_x86_64_maps_to_zero() {
        assert_eq!(arch_str_to_kernel_arch("x86_64").unwrap(), KernelArch::X86_64 as u8);
    }

    #[test]
    fn test_arch_aarch64_maps_to_one() {
        assert_eq!(arch_str_to_kernel_arch("aarch64").unwrap(), KernelArch::Aarch64 as u8);
    }

    #[test]
    fn test_arch_unknown_returns_error() {
        assert!(arch_str_to_kernel_arch("mips64").is_err());
    }

    #[test]
    fn test_build_skeleton_with_aarch64_arch() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("aarch64.rvf");
        let mut manifest = test_manifest();
        manifest.kernel.arch = "aarch64".into();
        let builder = ApplianceBuilder::new(manifest).unwrap();
        builder.build_skeleton(&output, None).unwrap();
        assert!(output.exists());
    }

    #[test]
    fn test_builder_new_generates_signing_key() {
        let manifest = test_manifest();
        let builder = ApplianceBuilder::new(manifest).unwrap();
        let verifying_key = builder.signing_key.verifying_key();
        assert_eq!(verifying_key.to_bytes().len(), 32);
    }

    #[test]
    fn test_build_skeleton_creates_rvf_file() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("testapp.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&output, None).unwrap();
        assert!(output.exists(), "testapp.rvf should exist after build_skeleton");
    }

    #[test]
    fn test_build_skeleton_embeds_kernel_seg() {
        let dir = tempdir().unwrap();
        let fake_kernel = dir.path().join("bzImage");
        std::fs::write(&fake_kernel, b"fake kernel bytes").unwrap();

        let output = dir.path().join("testapp.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&output, Some(&fake_kernel)).unwrap();
        assert!(output.exists(), "testapp.rvf should exist with kernel");
    }

    #[test]
    fn test_verify_passes_on_fresh_appliance() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("testapp.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&output, None).unwrap();
        builder.verify(&output).unwrap();
    }

    #[test]
    fn test_build_skeleton_manifest_retrievable() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("testapp.rvf");
        let manifest = test_manifest();
        let builder = ApplianceBuilder::new(manifest.clone()).unwrap();
        builder.build_skeleton(&output, None).unwrap();

        // Open store and extract cmdline (manifest JSON)
        let store = RvfStore::open_readonly(&output).unwrap();
        let (hdr_bytes, remainder) = store.extract_kernel().unwrap().unwrap();
        assert_eq!(hdr_bytes.len(), 128);

        // KernelHeader layout (see rvf-types/src/kernel.rs to_bytes):
        //   0x18..0x20: image_size u64 (LE)
        //   0x78..0x7C: cmdline_length u32 (LE)
        // Remainder = kernel_image(image_size bytes) || cmdline(cmdline_length bytes)
        let image_size = u64::from_le_bytes([
            hdr_bytes[0x18], hdr_bytes[0x19], hdr_bytes[0x1A], hdr_bytes[0x1B],
            hdr_bytes[0x1C], hdr_bytes[0x1D], hdr_bytes[0x1E], hdr_bytes[0x1F],
        ]) as usize;
        let cmdline_len = u32::from_le_bytes([
            hdr_bytes[0x78],
            hdr_bytes[0x79],
            hdr_bytes[0x7A],
            hdr_bytes[0x7B],
        ]) as usize;
        assert!(cmdline_len > 0, "manifest JSON cmdline must be non-empty");

        // cmdline follows the kernel image in the remainder
        let cmdline_bytes = &remainder[image_size..image_size + cmdline_len];
        let recovered: serde_json::Value = serde_json::from_slice(cmdline_bytes).unwrap();
        assert_eq!(
            recovered["project_id"].as_str().unwrap(),
            "test-id"
        );
    }
}
