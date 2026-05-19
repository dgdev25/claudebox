#[test]
#[ignore = "requires rvf-cli and host VM/toolchain prerequisites"]
fn test_upgrade_kernel_preserves_segments() {
    // claudebox init with session data in META_SEG
    // claudebox upgrade-kernel
    // Verify META_SEG identical to pre-upgrade (byte-for-byte)
    // Verify WITNESS_SEG has KernelUpgrade event
    // Verify kernel_built_at updated
}
