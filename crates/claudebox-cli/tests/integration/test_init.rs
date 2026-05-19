#[test]
#[ignore = "requires rvf-cli, clang, docker"]
fn test_init_single_language() {
    let dir = tempfile::tempdir().unwrap();
    let result = std::process::Command::new("cargo")
        .args(["run", "--bin", "claudebox", "--",
               "init", "myapp", "--lang", "node@22"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(result.status.success(),
        "init failed: {}", String::from_utf8_lossy(&result.stderr));
    assert!(dir.path().join("myapp.rvf").exists());
    // Verify no .rvf.tmp leftover
    assert!(!dir.path().join("myapp.rvf.tmp").exists());
}

#[test]
#[ignore = "requires rvf-cli, clang, docker"]
fn test_init_multi_language() {
    let dir = tempfile::tempdir().unwrap();
    let result = std::process::Command::new("cargo")
        .args(["run", "--bin", "claudebox", "--",
               "init", "polyglot", "--lang", "node@22,rust@1.87"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    // Verify Multi variant in MANIFEST_SEG
    // Verify union network policy has 5 unique domains
    let rvf = dir.path().join("polyglot.rvf");
    // TODO: inspect MANIFEST_SEG via rvf-cli
    assert!(rvf.exists());
}

#[test]
#[ignore = "requires rvf-cli, clang"]
fn test_init_failure_leaves_no_artefacts() {
    // Simulate a failure mid-init (invalid eBPF source)
    // Verify no .rvf or .rvf.tmp remains on disk after failure
    // This tests InitTransaction::drop cleanup
}

#[test]
#[ignore = "requires rvf-cli, clang, docker"]
fn test_init_kernel_from_existing_appliance() {
    // claudebox init newapp --lang node@22 --kernel-from existingapp.rvf
    // Verify init succeeds without calling Docker
    // Verify KERNEL_SEG in new appliance matches source
}
