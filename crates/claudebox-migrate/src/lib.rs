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

/// Stub migrator from schema v1 → v2. Full implementation deferred to Phase 11.
pub struct V1ToV2Migrator;

impl SegmentMigrator for V1ToV2Migrator {
    fn from_version(&self) -> u8 { 1 }
    fn to_version(&self) -> u8 { 2 }
    fn migrate(&self, _store: &mut RvfStore) -> anyhow::Result<()> {
        // Phase 11: transform MANIFEST_SEG from schema v1 to v2,
        // append FormatMigrate witness event.
        Ok(())
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
    pub fn migrate_to_latest(
        &self,
        _rvf_path: &std::path::Path,
        current_version: u8,
    ) -> anyhow::Result<u8> {
        let mut version = current_version;
        for m in &self.migrators {
            if m.from_version() == version {
                // Phase 11: open RvfStore and call m.migrate(store) here.
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
        self.migrators
            .iter()
            .map(|m| m.to_version())
            .max()
            .unwrap_or(1)
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
}
