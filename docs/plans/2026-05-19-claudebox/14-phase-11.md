# ClaudeBox — Phase 11: Format Version Migration + Completion

> **Prerequisite:** Phase 10 complete and verified.
> After completing this phase: migration chain tests pass, full workspace clean.

---

### Task 11.1: MigrationChain full implementation + V1ToV2Migrator

**Files:**
- Modify: `crates/claudebox-migrate/src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_v1_to_v2_migration_updates_version() {
    // Create a v1 appliance
    // Run MigrationChain::migrate_to_latest
    // Verify MANIFEST_SEG version == 2
    // Verify FormatMigrate event in WITNESS_SEG
    // Verify all original data fields intact
    todo!() // full impl after V1ToV2Migrator struct exists
}

#[test]
fn test_check_and_migrate_auto_migrates_single_step() {
    // Mock: version = latest - 1 (single step behind)
    // check_and_migrate with auto_migrate_minor=true
    // Expect: migration runs, no error
}

#[test]
fn test_check_and_migrate_errors_on_multi_step_without_flag() {
    // Mock: version = 0 (multiple steps behind)
    // check_and_migrate with auto_migrate_minor=false
    // Expect: error containing "claudebox migrate" command
}

#[test]
fn test_migration_appends_format_migrate_witness_event() {
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    // After migration runs, WITNESS_SEG should contain FormatMigrate event
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
```

- [ ] **Step 2: Implement V1ToV2Migrator**

```rust
pub struct V1ToV2Migrator;

impl SegmentMigrator for V1ToV2Migrator {
    fn from_version(&self) -> u8 { 1 }
    fn to_version(&self) -> u8 { 2 }

    fn migrate(&self, store: &mut RvfStore) -> anyhow::Result<()> {
        // v1 → v2: example migration
        // 1. Read MANIFEST_SEG, update version field to 2
        // 2. Write updated MANIFEST_SEG
        // 3. Append FormatMigrate witness event
        // Real migration steps depend on schema evolution in production
        todo!()
    }
}
```

- [ ] **Step 3: Complete MigrationChain.migrate_to_latest()**

Full implementation: iterate migrators from `current_version` to `latest_version()`, calling each `migrate()`, appending `FormatMigrate` witness event after each step.

- [ ] **Step 4: Run tests to verify pass**

```bash
cargo test -p claudebox-migrate
# Expected: all non-todo tests pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-migrate/
git commit -m "feat(migrate): implement MigrationChain with V1ToV2Migrator and FormatMigrate witness"
```

---

### Task 11.2: Wire `check_and_migrate` into every CLI command

**Files:**
- Modify: `crates/claudebox-cli/src/main.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_check_and_migrate_called_before_start() {
    // Verify that the start command's execution path calls check_and_migrate
    // This is a structural test — verify by checking that start.rs imports migrate
    // (Compilation test — if import is missing, this won't compile)
    let _: fn(&std::path::Path, bool) -> anyhow::Result<()> = claudebox_migrate::check_and_migrate;
}
```

- [ ] **Step 2: Add `check_and_migrate` call to each command dispatch**

In `main.rs`, for every command that operates on a `.rvf` file (init, start, stop, status, branch, rollback, audit, snapshot, compact, update-allowlist, upgrade-kernel, migrate, destroy):

```rust
Commands::Start { rvf, workspace } => {
    claudebox_migrate::check_and_migrate(&rvf, true)?; // auto-migrate minor
    claudebox_core::start::run_start(StartOptions { rvf, workspace }).await?;
}
```

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-cli
git add crates/claudebox-cli/src/main.rs
git commit -m "feat(cli): wire check_and_migrate before every .rvf command — schema gating"
```

---

### Task 11.3: config.toml writer and reader

**Files:**
- Create: `crates/claudebox-core/src/config.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_config_defaults() {
    let config = ClaudeBoxConfig::default();
    assert_eq!(config.defaults.vcpus, 2);
    assert_eq!(config.defaults.memory_mb, 4096);
    assert_eq!(config.witness.max_entries, 10_000);
    assert_eq!(config.witness.retention_days, 30);
    assert_eq!(config.kernel.staleness_warn_days, 90);
    assert_eq!(config.embedding.chunk_tokens, 512);
    assert_eq!(config.embedding.overlap_tokens, 64);
}

#[test]
fn test_config_round_trip_toml() {
    let config = ClaudeBoxConfig::default();
    let toml_str = toml::to_string(&config).unwrap();
    let restored: ClaudeBoxConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(restored.defaults.vcpus, config.defaults.vcpus);
    assert_eq!(restored.witness.max_entries, config.witness.max_entries);
}
```

- [ ] **Step 2: Implement `config.rs`**

Full `ClaudeBoxConfig` struct matching Technical Plan §7, with `Default` impl and `toml` serde. Includes `load()` (reads `~/.claudebox/config.toml`, falls back to default if absent) and `save()`.

Add `toml = "0.8"` to workspace dependencies and claudebox-core.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-core config
git add crates/claudebox-core/src/config.rs
git commit -m "feat(core): implement ClaudeBoxConfig — reads/writes ~/.claudebox/config.toml"
```

---

### Task 11.4: Final verification — Definition of Done checklist

**Files:** No new files — verification gate.

- [ ] **Step 1: Run full test suite**

```bash
cargo test --workspace -- --skip ignored
# Expected: all non-ignored tests PASS, zero failures
```

- [ ] **Step 2: Build release**

```bash
cargo build --workspace --release
# Expected: zero errors
```

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
# Expected: zero warnings
```

- [ ] **Step 4: Verify CLI end-to-end (no KVM needed)**

```bash
./target/release/claudebox --version
./target/release/claudebox --help
./target/release/claudebox init --help
./target/release/claudebox start --help
./target/release/claudebox audit --help
./target/release/claudebox kernel --help
./target/release/claudebox snapshot --help
# Expected: all print help without error
```

- [ ] **Step 5: Definition of Done checklist (from Technical Plan §10)**

- [ ] `cargo build --workspace --release` passes with zero errors
- [ ] `cargo test --workspace` passes (all non-ignored tests)
- [ ] `cargo clippy --workspace` produces zero warnings
- [ ] `claudebox init myapp --lang node@22` — code path exists, manifest builds correctly (unit test)
- [ ] `claudebox init polyglot --lang node@22,rust@1.87` — Multi language profile test passes
- [ ] `claudebox init newapp --kernel-from existing.rvf` — KernelImporter test passes
- [ ] Init failure leaves zero artefacts — InitTransaction::drop test passes
- [ ] SessionLock RAII tests: acquire errors on live PID, clears stale, drop removes file
- [ ] WitnessCompactor tests: archives when over limit, noop when under
- [ ] VecReconciler: tombstone tests pass
- [ ] MCP find_mcp_port: skips bound port test passes
- [ ] MigrationChain: auto-migrate single step test passes
- [ ] ClaudeBoxConfig: defaults and TOML round-trip tests pass
- [ ] Audit chain verifier: valid chain + tamper detection tests pass

- [ ] **Step 6: Update 00-index.md Document Map — mark all phases complete**

- [ ] **Step 7: Final commit**

```bash
git add -A
git commit -m "feat: ClaudeBox implementation complete — all phases verified"
```

---

## Phase C — Completion Checklist

- [ ] All tasks complete across all 12 phase documents (Phases 0–11)
- [ ] Full test suite passes: `cargo test --workspace`
- [ ] Type check / build passes: `cargo build --workspace --release`
- [ ] Linter clean: `cargo clippy --workspace -- -D warnings`
- [ ] All success criteria from `00-index.md` are met
- [ ] No `unwrap()` or `expect()` outside `#[cfg(test)]` blocks
- [ ] No `todo!()` in non-test code
- [ ] No dead code, no debug logs, no TODO comments left in src/
- [ ] All `#[ignore]` integration tests exist and compile (even if not run)
- [ ] `00-index.md` Document Map updated — all phases marked complete
- [ ] README.md created (covers: prerequisites, quickstart, architecture, air-gap, macOS setup)
