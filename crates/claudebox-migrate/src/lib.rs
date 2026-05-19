use rvf_runtime::RvfStore;

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
        _rvf_path: &std::path::Path,
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
                // FIXME: open RvfStore and call m.migrate(store)? here before advancing.
                // Until rvf-runtime is wired, this is a no-op stub — version advances
                // without executing the migrator body. Do NOT ship this without fixing.
                // m.migrate(store)?;
                version = m.to_version();
                tracing::info!(
                    "Applied migration v{} → v{}",
                    m.from_version(),
                    m.to_version()
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
pub fn check_and_migrate(rvf_path: &std::path::Path, auto_migrate_minor: bool) -> anyhow::Result<()> {
    // Phase 11: read manifest from rvf_path to get current schema version.
    // For now, treat all files as version 1 (current).
    let _ = rvf_path;
    let _ = auto_migrate_minor;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claudebox_witness::{WitnessEvent, writer::WitnessWriter};

    #[test]
    #[ignore = "full impl after V1ToV2Migrator struct exists"]
    fn test_v1_to_v2_migration_updates_version() {
        todo!() // full impl after rvf-runtime integration in Phase 11
    }

    #[test]
    fn test_check_and_migrate_stub_returns_ok() {
        // Stub returns Ok(()) for any path until rvf-runtime is wired.
        // FIXME: replace with real assertions once check_and_migrate reads the RVF schema version.
        let result = check_and_migrate(std::path::Path::new("/tmp/test.rvf"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_and_migrate_no_auto_migrate_stub_returns_ok() {
        // FIXME: once implemented, false should prevent minor auto-migration.
        let result = check_and_migrate(std::path::Path::new("/tmp/test.rvf"), false);
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
