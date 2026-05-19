use std::path::{Path, PathBuf};

use rvf_runtime::RvfStore;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/rvf")
}

fn fixture_paths() -> Vec<PathBuf> {
    vec![
        fixtures_dir().join("basic_store.rvf"),
        fixtures_dir().join("compacted.rvf"),
        fixtures_dir().join("ebpf_accelerator.rvf"),
        fixtures_dir().join("mcp_in_rvf.rvf"),
        fixtures_dir().join("claude_code_appliance_v1.rvf"),
    ]
}

#[test]
fn test_curated_rvf_fixtures_are_readable() {
    for fixture in fixture_paths() {
        assert!(fixture.exists(), "missing fixture: {}", fixture.display());
        let size = std::fs::metadata(&fixture)
            .unwrap_or_else(|e| panic!("failed to stat {}: {e}", fixture.display()))
            .len();
        assert!(size > 0, "fixture is empty: {}", fixture.display());

        let store = RvfStore::open_readonly(&fixture)
            .unwrap_or_else(|e| panic!("failed to open {}: {e:?}", fixture.display()));
        assert!(
            store.dimension() > 0,
            "fixture has invalid dimension: {}",
            fixture.display()
        );
    }
}
