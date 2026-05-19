#[test]
#[ignore = "requires rvf-cli"]
fn test_snapshot_export_is_self_contained() {
    // claudebox snapshot create pre-refactor
    // claudebox snapshot export pre-refactor --output ./snap.rvf
    // Verify snap.rvf passes rvf verify-witness
    // Verify snap.rvf has no parent dependency (bootable independently)
}
