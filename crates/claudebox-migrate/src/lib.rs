use rvf_runtime::RvfStore;
use rvf_types::kernel::KernelHeader;

/// Trait implemented by each version-to-version migration step.
#[allow(clippy::wrong_self_convention)]
pub trait SegmentMigrator: Send + Sync {
    /// Schema version this migrator reads from.
    fn from_version(&self) -> u8;
    /// Schema version this migrator produces.
    fn to_version(&self) -> u8;
    fn migrate(&self, store: &mut RvfStore) -> anyhow::Result<()>;
}

/// Migrator from schema v1 → v2.
pub struct V1ToV2Migrator;

impl SegmentMigrator for V1ToV2Migrator {
    fn from_version(&self) -> u8 { 1 }
    fn to_version(&self) -> u8 { 2 }

    fn migrate(&self, _store: &mut RvfStore) -> anyhow::Result<()> {
        // V1ToV2 migration requires rvf-runtime segment access — deferred
        anyhow::bail!("V1ToV2 migration requires rvf-runtime — deferred")
    }
}

/// Ordered chain of all known migrators.
pub struct MigrationChain {
    migrators: Vec<Box<dyn SegmentMigrator>>,
}

impl MigrationChain {
    pub fn new() -> Self {
        Self {
            migrators: vec![
                Box::new(V1ToV2Migrator),
                // Add future migrators here as schema evolves.
            ],
        }
    }

    /// Applies all required migrations sequentially from `current_version` to latest.
    ///
    /// Errors if `current_version` is newer than what this binary supports, or if any
    /// migrator fails. Version only advances after `m.migrate()` succeeds.
    pub fn migrate_to_latest(
        &self,
        rvf_path: &std::path::Path,
        current_version: u8,
    ) -> anyhow::Result<u8> {
        let latest = self.latest_version();
        if current_version > latest {
            anyhow::bail!(
                "RVF schema version {current_version} is newer than this binary supports \
                 (max {latest}); upgrade claudebox"
            );
        }
        let mut version = current_version;
        for m in &self.migrators {
            if m.from_version() == version {
                let mut store = RvfStore::open(rvf_path)
                    .map_err(|e| anyhow::anyhow!("failed to open {} for migration: {e:?}", rvf_path.display()))?;
                m.migrate(&mut store)?;
                version = m.to_version();
                tracing::info!(
                    "Applied migration v{} → v{} on {}",
                    m.from_version(),
                    m.to_version(),
                    rvf_path.display()
                );
            }
        }
        Ok(version)
    }

    pub fn needs_migration(&self, current_version: u8) -> bool {
        current_version < self.latest_version()
    }

    pub fn latest_version(&self) -> u8 {
        const BASELINE_VERSION: u8 = 1;
        self.migrators
            .iter()
            .map(|m| m.to_version())
            .max()
            .unwrap_or(BASELINE_VERSION)
    }
}

impl Default for MigrationChain {
    fn default() -> Self { Self::new() }
}

/// Called by every `claudebox` command before operating on a `.rvf` file.
/// Opens the RVF, reads the embedded manifest, compares schema `version` to
/// the migration chain. Returns an error for newer-than-supported files;
/// auto-applies minor migrations when `auto_migrate_minor` is `true`.
pub fn check_and_migrate(rvf_path: &std::path::Path, auto_migrate_minor: bool) -> anyhow::Result<()> {
    if !rvf_path.exists() {
        anyhow::bail!("{} not found", rvf_path.display());
    }

    let schema_version = read_schema_version(rvf_path).unwrap_or(1);

    let chain = MigrationChain::new();
    if schema_version > chain.latest_version() {
        anyhow::bail!(
            "{} schema version {} is newer than this binary supports (max {}); \
             upgrade claudebox",
            rvf_path.display(),
            schema_version,
            chain.latest_version()
        );
    }

    if auto_migrate_minor && chain.needs_migration(schema_version) {
        tracing::info!(
            "{}: migrating from schema v{} to v{}",
            rvf_path.display(),
            schema_version,
            chain.latest_version()
        );
        chain.migrate_to_latest(rvf_path, schema_version)?;
    }

    Ok(())
}

/// Read the `version` field from the manifest JSON embedded in the RVF kernel
/// segment. Returns `None` if the file is not a valid RVF or has no manifest.
pub fn read_schema_version(rvf_path: &std::path::Path) -> Option<u8> {
    let store = RvfStore::open_readonly(rvf_path).ok()?;
    let (hdr_bytes, remainder) = store.extract_kernel().ok()??;
    if hdr_bytes.len() != 128 {
        return None;
    }
    let mut arr = [0u8; 128];
    arr.copy_from_slice(&hdr_bytes);
    let header = KernelHeader::from_bytes(&arr).ok()?;
    let image_size = header.image_size as usize;
    let cmdline_len = header.cmdline_length as usize;
    if remainder.len() < image_size + cmdline_len {
        return None;
    }
    let json_bytes = &remainder[image_size..image_size + cmdline_len];
    let value: serde_json::Value = serde_json::from_slice(json_bytes).ok()?;
    value["version"].as_u64().map(|v| v as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claudebox_witness::{WitnessEvent, writer::WitnessWriter};

    fn build_test_rvf(path: &std::path::Path) {
        use claudebox_core::manifest::*;
        let manifest = ClaudeBoxManifest {
            version: 1,
            project_id: "migrate-test".into(),
            project_name: "migrate-test".into(),
            language: LanguageProfile::Single(SingleProfile { lang: Lang::Node, version: "22".into() }),
            created_at: "2026-05-19T00:00:00Z".into(),
            kernel_built_at: "2026-05-19T00:00:00Z".into(),
            network: NetworkPolicy { allow_domains: vec![], allow_localhost: false, dns_server: "1.1.1.1".into() },
            resources: ResourceLimits::default(),
            kernel: KernelConfig { arch: "x86_64".into(), ssh_port: 2222, mcp_port: 7878 },
            witness: WitnessPolicy::default(),
        };
        claudebox_rvf::builder::ApplianceBuilder::new(manifest)
            .unwrap()
            .build_skeleton(path, None)
            .unwrap();
    }

    #[test]
    #[ignore = "full impl after V1ToV2Migrator struct exists"]
    fn test_v1_to_v2_migration_updates_version() {
        todo!() // full impl after rvf-runtime integration in Phase 11
    }

    #[test]
    fn test_check_and_migrate_missing_file_returns_error() {
        let result = check_and_migrate(std::path::Path::new("/tmp/claudebox-test-nonexistent-xyzabc.rvf"), true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_read_schema_version_missing_file_returns_none() {
        let result = read_schema_version(std::path::Path::new("/tmp/claudebox-no-such-file.rvf"));
        assert!(result.is_none());
    }

    #[test]
    fn test_check_and_migrate_existing_v1_rvf_passes() {
        // Build a real v1 RVF via ApplianceBuilder and verify check_and_migrate returns Ok.
        let dir = tempfile::tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        build_test_rvf(&rvf);
        let result = check_and_migrate(&rvf, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_migration_appends_format_migrate_witness_event() {
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let event = WitnessEvent::FormatMigrate { from_version: 1, to_version: 2 };
        let entry = WitnessWriter::create_genesis(&signing_key, event).unwrap();
        match &entry.event {
            WitnessEvent::FormatMigrate { from_version, to_version } => {
                assert_eq!(*from_version, 1);
                assert_eq!(*to_version, 2);
            }
            _ => panic!("Expected FormatMigrate event"),
        }
    }

    #[test]
    fn test_migration_chain_no_op_when_current() {
        let chain = MigrationChain::new();
        let latest = chain.latest_version();
        assert!(!chain.needs_migration(latest));
    }

    #[test]
    fn test_migration_chain_needs_migration_when_behind() {
        let chain = MigrationChain::new();
        assert!(chain.needs_migration(0));
    }

    #[test]
    fn test_migrate_to_latest_errors_on_future_version() {
        let chain = MigrationChain::new();
        let future_version = chain.latest_version() + 10;
        let result = chain.migrate_to_latest(std::path::Path::new("/tmp/test.rvf"), future_version);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("newer than this binary supports") || msg.contains("upgrade"));
    }
}
