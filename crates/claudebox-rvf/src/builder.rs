use std::path::Path;

use claudebox_core::manifest::ClaudeBoxManifest;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rvf_runtime::RvfStore;

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

    /// Write an empty RVF skeleton to `output_path`. Full impl in Phase 4.
    pub fn build_skeleton(&self, _output_path: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    /// Embed a kernel image into the RVF store. Full impl in Phase 4.
    pub fn embed_kernel(
        &self,
        _store: &mut RvfStore,
        _kernel_path: &Path,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Embed an eBPF program into the RVF store. Full impl in Phase 4.
    pub fn embed_ebpf(
        &self,
        _store: &mut RvfStore,
        _ebpf_path: &Path,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Append a genesis witness entry to the RVF store. Full impl in Phase 4.
    pub fn write_genesis_witness(&self, _store: &mut RvfStore) -> anyhow::Result<()> {
        Ok(())
    }

    /// Verify the appliance at `rvf_path`. Full impl in Phase 4.
    pub fn verify(&self, _rvf_path: &Path) -> anyhow::Result<()> {
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
        // Signing key is generated — verifying key must be extractable
        let verifying_key = builder.signing_key.verifying_key();
        assert_eq!(verifying_key.to_bytes().len(), 32);
    }
}
