#[test]
#[ignore = "requires rvf-cli"]
fn test_compaction_archives_old_entries() {
    // 1. Create appliance with 12,000 witness entries
    // 2. Run WitnessCompactor (max_entries = 10,000)
    // 3. Verify hot chain has <= 10,000 entries
    // 4. Verify archive .rvf contains the excess
    // 5. Verify both chains pass `rvf verify-witness`
}

#[test]
#[ignore = "requires rvf-cli"]
fn test_audit_archive_flag() {
    // claudebox audit myapp.rvf --archive 2026-04
    // Verify reads the monthly archive file
    // Verify chain integrity verified for archived entries
}
