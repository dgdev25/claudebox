use std::io::BufWriter;
use std::path::Path;

use claudebox_core::manifest::ClaudeBoxManifest;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;

use crate::format::{tag, RvfWriter};
use crate::transaction::InitTransaction;

/// Builds a ClaudeBox RVF appliance from a manifest and a freshly-generated Ed25519
/// signing key. The signing key is generated at construction time so that every
/// appliance has its own unique identity.
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

    /// Write the appliance skeleton to `output_path` atomically.
    ///
    /// Always writes:
    /// - `MANIFEST` seg — JSON-encoded `ClaudeBoxManifest`
    /// - `CRYPTO`   seg — Ed25519 pubkey (32 bytes) + signature over manifest (64 bytes)
    ///
    /// Optionally writes:
    /// - `KERNEL`   seg — raw bytes from `kernel_path` (if `Some`)
    ///
    /// Uses [`InitTransaction`] for atomic `.rvf.tmp` → `.rvf` rename with
    /// EXDEV cross-filesystem fallback.
    pub fn build_skeleton(
        &self,
        output_path: &Path,
        kernel_path: Option<&Path>,
    ) -> anyhow::Result<()> {
        let project_name = &self.manifest.project_name;
        let dir = output_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("output_path has no parent directory"))?;

        let tx = InitTransaction::new(project_name, dir)?;

        {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(tx.tmp_path())
                .map_err(|e| anyhow::anyhow!("failed to create staging file: {e}"))?;
            let mut writer = RvfWriter::new(BufWriter::new(file))
                .map_err(|e| anyhow::anyhow!("failed to write RVF header: {e}"))?;

            // MANIFEST segment
            let manifest_json = serde_json::to_vec(&self.manifest)
                .map_err(|e| anyhow::anyhow!("failed to serialize manifest: {e}"))?;
            writer
                .write_segment(tag::MANIFEST, &manifest_json)
                .map_err(|e| anyhow::anyhow!("failed to write MANIFEST seg: {e}"))?;

            // CRYPTO segment: pubkey || Ed25519 signature over manifest JSON
            let signature = self.signing_key.sign(&manifest_json);
            let pubkey = self.signing_key.verifying_key();
            let mut crypto = Vec::with_capacity(32 + 64);
            crypto.extend_from_slice(pubkey.as_bytes());
            crypto.extend_from_slice(&signature.to_bytes());
            writer
                .write_segment(tag::CRYPTO, &crypto)
                .map_err(|e| anyhow::anyhow!("failed to write CRYPTO seg: {e}"))?;

            // Optional KERNEL segment
            if let Some(kp) = kernel_path {
                let kernel_bytes = std::fs::read(kp).map_err(|e| {
                    anyhow::anyhow!("failed to read kernel from {}: {e}", kp.display())
                })?;
                writer
                    .write_segment(tag::KERNEL, &kernel_bytes)
                    .map_err(|e| anyhow::anyhow!("failed to write KERNEL seg: {e}"))?;
            }

            writer
                .finalize()
                .map_err(|e| anyhow::anyhow!("failed to finalize RVF file: {e}"))?;
        }

        tx.commit()
    }

    /// Append an eBPF segment to an existing `.rvf` file.
    ///
    /// Full implementation in Phase 5 (eBPF compilation pipeline).
    pub fn embed_ebpf(&self, _rvf_path: &Path, _ebpf_path: &Path) -> anyhow::Result<()> {
        anyhow::bail!("embed_ebpf not yet implemented — deferred to Phase 5 (eBPF pipeline)")
    }

    /// Append a genesis witness entry to an existing `.rvf` file.
    ///
    /// Full implementation in Phase 5 (witness chain integration).
    pub fn write_genesis_witness(&self, _rvf_path: &Path) -> anyhow::Result<()> {
        anyhow::bail!(
            "write_genesis_witness not yet implemented — deferred to Phase 5 (witness chain)"
        )
    }

    /// Verify the appliance at `rvf_path`: read MANIFEST and CRYPTO segs,
    /// reconstruct the Ed25519 signature check.
    pub fn verify(&self, rvf_path: &Path) -> anyhow::Result<()> {
        use crate::format::RvfFile;
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let rvf = RvfFile::open(rvf_path)?;

        let manifest_bytes = rvf
            .find(tag::MANIFEST)
            .ok_or_else(|| anyhow::anyhow!("MANIFEST segment missing from {}", rvf_path.display()))?;
        let crypto_bytes = rvf
            .find(tag::CRYPTO)
            .ok_or_else(|| anyhow::anyhow!("CRYPTO segment missing from {}", rvf_path.display()))?;

        anyhow::ensure!(
            crypto_bytes.len() == 96,
            "CRYPTO segment is {} bytes; expected 96 (32 pubkey + 64 sig)",
            crypto_bytes.len()
        );

        let pubkey = VerifyingKey::from_bytes(
            crypto_bytes[..32]
                .try_into()
                .expect("slice is exactly 32 bytes"),
        )
        .map_err(|e| anyhow::anyhow!("invalid Ed25519 public key: {e}"))?;

        let sig_bytes: [u8; 64] = crypto_bytes[32..]
            .try_into()
            .expect("slice is exactly 64 bytes");
        let signature = Signature::from_bytes(&sig_bytes);

        pubkey
            .verify(manifest_bytes, &signature)
            .map_err(|_| anyhow::anyhow!("CRYPTO signature verification failed — appliance may be tampered"))
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
    fn test_build_skeleton_produces_valid_rvf() {
        use crate::format::{tag, RvfFile};

        let dir = tempdir().unwrap();
        let output = dir.path().join("testapp.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&output, None).unwrap();

        let rvf = RvfFile::open(&output).unwrap();
        assert!(rvf.find(tag::MANIFEST).is_some(), "MANIFEST seg must be present");
        assert!(rvf.find(tag::CRYPTO).is_some(), "CRYPTO seg must be present");
        assert!(rvf.find(tag::KERNEL).is_none(), "KERNEL seg absent when kernel_path=None");
    }

    #[test]
    fn test_build_skeleton_with_kernel_embeds_kernel_seg() {
        use crate::format::{tag, RvfFile};

        let dir = tempdir().unwrap();
        let fake_kernel = dir.path().join("bzImage");
        std::fs::write(&fake_kernel, b"fake kernel bytes").unwrap();

        let output = dir.path().join("testapp.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&output, Some(&fake_kernel)).unwrap();

        let rvf = RvfFile::open(&output).unwrap();
        assert_eq!(rvf.find(tag::KERNEL).unwrap(), b"fake kernel bytes");
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
    fn test_verify_fails_on_tampered_manifest() {
        use crate::format::RvfFile;
        use std::io::{BufWriter, Cursor};

        let dir = tempdir().unwrap();
        let output = dir.path().join("testapp.rvf");
        let builder = ApplianceBuilder::new(test_manifest()).unwrap();
        builder.build_skeleton(&output, None).unwrap();

        // Tamper: read all segs, replace MANIFEST payload, rewrite
        let rvf = RvfFile::open(&output).unwrap();
        let tampered_output = dir.path().join("tampered.rvf");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tampered_output)
            .unwrap();
        let mut writer = RvfWriter::new(BufWriter::new(file)).unwrap();
        for seg in &rvf.segments {
            if &seg.tag == tag::MANIFEST {
                writer.write_segment(&seg.tag, b"tampered manifest content").unwrap();
            } else {
                writer.write_segment(&seg.tag, &seg.payload).unwrap();
            }
        }
        writer.finalize().unwrap();

        let result = builder.verify(&tampered_output);
        assert!(result.is_err(), "verify must fail on tampered manifest");
    }
}
