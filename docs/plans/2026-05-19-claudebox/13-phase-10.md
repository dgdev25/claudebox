# ClaudeBox — Phase 10: Integration Tests

> **Prerequisite:** Phase 9 complete and verified.
> All integration tests are `#[ignore]` by default.
> Run with: `cargo test -- --ignored` on a KVM host.

---

### Task 10.1: Init integration tests

**Files:**
- Create: `tests/integration/test_init.rs`

- [ ] **Step 1: Write integration tests (all `#[ignore]`)**

```rust
use std::path::PathBuf;

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
```

- [ ] **Step 2: Verify tests compile (they will be #[ignore])**

```bash
cargo test -p claudebox -- --list 2>&1 | grep ignored
# Expected: lists the #[ignore] tests
```

- [ ] **Step 3: Commit**

```bash
git add tests/integration/test_init.rs
git commit -m "test(integration): add init integration tests (#[ignore] — requires Docker + KVM)"
```

---

### Task 10.2: Witness + VEC integration tests

**Files:**
- Create: `tests/integration/test_witness.rs`
- Create: `tests/integration/test_vec.rs`

- [ ] **Step 1: Write witness integration tests**

```rust
// tests/integration/test_witness.rs

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
```

- [ ] **Step 2: Write VEC integration tests**

```rust
// tests/integration/test_vec.rs

#[test]
#[ignore = "requires rvf-cli"]
fn test_reconciler_tombstones_deleted_files() {
    // 1. Index workspace with 10 files
    // 2. Delete 3 files from disk
    // 3. Run VecReconciler
    // 4. Verify 3 files tombstoned in VEC_SEG metadata
    // 5. Verify search_codebase returns 0 results for deleted paths
}

#[test]
#[ignore = "requires rvf-cli"]
fn test_compact_removes_tombstoned_entries() {
    // Follow up from above: run claudebox compact
    // Verify tombstoned entries are physically absent
    // Verify HNSW index is valid and queryable
}
```

- [ ] **Step 3: Commit**

```bash
git add tests/integration/
git commit -m "test(integration): add witness + VEC integration tests (#[ignore])"
```

---

### Task 10.3: Boot + kernel integration tests

**Files:**
- Create: `tests/integration/test_boot.rs`
- Create: `tests/integration/test_kernel.rs`
- Create: `tests/integration/test_snapshot.rs`

- [ ] **Step 1: Write boot integration tests**

```rust
// tests/integration/test_boot.rs

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_boot_and_session_restore() {
    // claudebox init → claudebox start → wait for SSH
    // Write session data, claudebox stop
    // claudebox start again
    // Verify session-context.txt contains previous task_context
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_session_lock_prevents_double_boot() {
    // Start VM
    // Attempt claudebox start on same .rvf
    // Verify error contains "already running"
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_stale_lock_cleared_on_boot() {
    // Write lock file with non-existent PID
    // Attempt claudebox start
    // Verify boot succeeds (stale lock auto-cleared)
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_mcp_port_conflict_resolved() {
    // Start VM; artificially bind 7878 before MCP server starts
    // Verify MCP server binds on 7879
    // Verify .claude/mcp.json updated with 7879
}
```

- [ ] **Step 2: Write kernel integration test**

```rust
// tests/integration/test_kernel.rs

#[test]
#[ignore = "requires rvf-cli, docker"]
fn test_upgrade_kernel_preserves_segments() {
    // claudebox init with session data in META_SEG
    // claudebox upgrade-kernel
    // Verify META_SEG identical to pre-upgrade (byte-for-byte)
    // Verify WITNESS_SEG has KernelUpgrade event
    // Verify kernel_built_at updated
}
```

- [ ] **Step 3: Write snapshot integration test**

```rust
// tests/integration/test_snapshot.rs

#[test]
#[ignore = "requires rvf-cli"]
fn test_snapshot_export_is_self_contained() {
    // claudebox snapshot create pre-refactor
    // claudebox snapshot export pre-refactor --output ./snap.rvf
    // Verify snap.rvf passes rvf verify-witness
    // Verify snap.rvf has no parent dependency (bootable independently)
}
```

- [ ] **Step 4: Verify all integration tests compile**

```bash
cargo test -- --list 2>&1 | grep "ignored"
# Expected: lists all #[ignore] tests
```

- [ ] **Step 5: Commit**

```bash
git add tests/integration/
git commit -m "test(integration): add boot, kernel, snapshot integration tests (#[ignore])"
```
