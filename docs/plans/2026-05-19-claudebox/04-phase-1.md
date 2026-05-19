# ClaudeBox — Phase 1: Foundation — Core Types + Manifest

> **Prerequisite:** Phase 0 complete and verified (`cargo test -p claudebox-core` passes).
> After completing this phase: all manifest round-trip tests pass, clippy clean.

---

### Task 1.1: ClaudeBoxManifest and all supporting types

**Files:**
- Create: `crates/claudebox-core/src/manifest.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_round_trip() {
        let manifest = ClaudeBoxManifest {
            version: 1,
            project_id: "abc-123".into(),
            project_name: "myapp".into(),
            language: LanguageProfile::Single(SingleProfile { lang: Lang::Node, version: "22".into() }),
            created_at: "2026-05-19T00:00:00Z".into(),
            kernel_built_at: "2026-05-19T00:00:00Z".into(),
            network: NetworkPolicy { allow_domains: vec!["registry.npmjs.org".into()],
                allow_localhost: true, dns_server: "1.1.1.1".into() },
            resources: ResourceLimits::default(),
            kernel: KernelConfig { arch: "x86_64".into(), ssh_port: 2222, mcp_port: 7878 },
            witness: WitnessPolicy::default(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: ClaudeBoxManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.project_id, "abc-123");
        assert_eq!(back.kernel.mcp_port, 7878);
    }

    #[test]
    fn test_single_language_node_domains() {
        let profile = LanguageProfile::Single(SingleProfile { lang: Lang::Node, version: "22".into() });
        let policy = NetworkPolicy::for_profiles(&profile.profiles());
        assert_eq!(policy.allow_domains.len(), 2);
        assert!(policy.allow_domains.contains(&"registry.npmjs.org".to_string()));
        assert!(policy.allow_domains.contains(&"nodejs.org".to_string()));
    }

    #[test]
    fn test_multi_language_union_deduplication() {
        let profile = LanguageProfile::Multi(vec![
            SingleProfile { lang: Lang::Node, version: "22".into() },
            SingleProfile { lang: Lang::Rust, version: "1.87".into() },
        ]);
        let policy = NetworkPolicy::for_profiles(&profile.profiles());
        // node: 2 domains, rust: 3 domains = 5 unique
        assert_eq!(policy.allow_domains.len(), 5);
        // No duplicates
        let mut sorted = policy.allow_domains.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), 5);
    }

    #[test]
    fn test_witness_policy_defaults() {
        let policy = WitnessPolicy::default();
        assert_eq!(policy.max_entries, 10_000);
        assert_eq!(policy.retention_days, 30);
    }

    #[test]
    fn test_kernel_config_default_mcp_port() {
        // Ensure mcp_port is 7878, NOT 8080 (BS-9 remediation)
        let config = KernelConfig { arch: "x86_64".into(), ssh_port: 2222, mcp_port: 7878 };
        assert_eq!(config.mcp_port, 7878);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p claudebox-core manifest -- --nocapture
# Expected: FAIL — types not defined
```

- [ ] **Step 3: Implement `manifest.rs`**

Full implementation exactly as specified in Technical Plan §3.1, including:
- `ClaudeBoxManifest` struct with all fields
- `LanguageProfile::Single` / `Multi` enum
- `SingleProfile { lang: Lang, version: String }`
- `Lang` enum: Node, Python, Rust, Go
- `LanguageProfile::profiles()` helper
- `NetworkPolicy` with `for_profiles()` and `domains_for_lang()`
- `ResourceLimits` with `Default` (vcpus=2, memory_mb=4096, disk_gb=20, network_mbps=100)
- `KernelConfig { arch, ssh_port: 2222, mcp_port: 7878 }`
- `WitnessPolicy` with `Default` (max_entries=10_000, retention_days=30)

- [ ] **Step 4: Run tests to verify all pass**

```bash
cargo test -p claudebox-core manifest
# Expected: 5/5 tests pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-core/src/manifest.rs
git commit -m "feat(core): implement ClaudeBoxManifest and all supporting types"
```

---

### Task 1.2: claudebox-witness crate — WitnessEntry types

**Files:**
- Create: `crates/claudebox-witness/Cargo.toml`
- Create: `crates/claudebox-witness/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_witness_event_all_variants_serialise() {
    let events = vec![
        WitnessEvent::Boot { project_id: "x".into() },
        WitnessEvent::Shutdown { reason: "graceful".into() },
        WitnessEvent::KernelUpgrade { from_hash: "a".into(), to_hash: "b".into() },
        WitnessEvent::WitnessCompact { entries_archived: 100, archive_path: "/tmp/x".into() },
        WitnessEvent::VecReconcile { files_removed: 3 },
        WitnessEvent::FormatMigrate { from_version: 1, to_version: 2 },
    ];
    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        assert!(!json.is_empty());
    }
}
```

- [ ] **Step 2: Create `crates/claudebox-witness/Cargo.toml`**

```toml
[package]
name = "claudebox-witness"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
sha3 = { workspace = true }
ed25519-dalek = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
```

- [ ] **Step 3: Implement `crates/claudebox-witness/src/lib.rs`**

Full `WitnessEntry` and `WitnessEvent` enum as specified in Technical Plan §3.3, including ALL variants: Boot, Shutdown, Command, FileWrite, FileDelete, NetworkRequest, PackageInstall, Snapshot, Branch, Rollback, KernelUpgrade, WitnessCompact, VecReconcile, FormatMigrate.

- [ ] **Step 4: Run tests and verify pass**

```bash
cargo test -p claudebox-witness
# Expected: PASS
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-witness/
git commit -m "feat(witness): add WitnessEntry and WitnessEvent types — all variants"
```

---

### Task 1.3: claudebox-meta crate — SessionState

**Files:**
- Create: `crates/claudebox-meta/Cargo.toml`
- Create: `crates/claudebox-meta/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_session_state_prompt_prefix() {
    let state = SessionState {
        last_boot: Some("2026-05-19T09:00:00Z".into()),
        working_dir: "/workspace/src".into(),
        task_context: "Implementing JWT".into(),
        scratchpad: "Use axum extractor".into(),
        history: vec![
            HistoryEntry { ts: "2026-05-19T09:00:01Z".into(),
                kind: HistoryEntryKind::Command, value: "cargo build".into() },
        ],
        installed_packages: vec!["axum@0.7".into()],
        open_files: vec!["src/main.rs".into()],
    };
    let prefix = state.to_claude_prompt_prefix();
    assert!(prefix.contains("ClaudeBox Session Context"));
    assert!(prefix.contains("Implementing JWT"));
    assert!(prefix.contains("cargo build"));
}

#[test]
fn test_session_state_default() {
    let state = SessionState::default();
    assert!(state.last_boot.is_none());
    assert!(state.history.is_empty());
}
```

- [ ] **Step 2: Create `crates/claudebox-meta/Cargo.toml`**

```toml
[package]
name = "claudebox-meta"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
claudebox-witness = { path = "../claudebox-witness" }
```

- [ ] **Step 3: Implement `crates/claudebox-meta/src/lib.rs`**

Full `SessionState`, `HistoryEntry`, `HistoryEntryKind` as in Technical Plan §3.2, including `to_claude_prompt_prefix()` returning the formatted context string.

- [ ] **Step 4: Run tests to verify pass**

```bash
cargo test -p claudebox-meta
# Expected: 2/2 pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-meta/
git commit -m "feat(meta): implement SessionState with to_claude_prompt_prefix"
```

---

### Task 1.4: claudebox-migrate crate — SegmentMigrator trait stub

**Files:**
- Create: `crates/claudebox-migrate/Cargo.toml`
- Create: `crates/claudebox-migrate/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_migration_chain_no_op_when_current() {
    let chain = MigrationChain::new();
    // If current version == latest version, needs_migration returns false
    let latest = chain.latest_version();
    assert!(!chain.needs_migration(latest));
}

#[test]
fn test_migration_chain_needs_migration_when_behind() {
    let chain = MigrationChain::new();
    assert!(chain.needs_migration(0)); // version 0 is always behind
}
```

- [ ] **Step 2: Create crate and implement trait stub**

Full `SegmentMigrator` trait, `MigrationChain` struct, and `check_and_migrate()` function as in Technical Plan §Phase 11. Include `V1ToV2Migrator` stub (full impl deferred to Phase 11).

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-migrate
git add crates/claudebox-migrate/
git commit -m "feat(migrate): add SegmentMigrator trait and MigrationChain stub"
```

---

### Task 1.5: Full workspace compile + clippy gate

**Files:** No new files — verification only

- [ ] **Step 1: Create skeleton crates for remaining members**

Create minimal `Cargo.toml` + `src/lib.rs` (or `src/main.rs`) for:
- `crates/claudebox-rvf/` — lib crate
- `crates/claudebox-firecracker/` — lib crate
- `crates/claudebox-ebpf/` — lib crate
- `crates/claudebox-vec/` — lib crate
- `crates/claudebox-logd/` — binary crate (src/main.rs with `fn main() {}`)
- `crates/claudebox-cli/` — binary crate (src/main.rs with `fn main() {}`)

Each `Cargo.toml` needs at minimum:
```toml
[package]
name = "claudebox-XXX"
version = "0.1.0"
edition = "2021"
```

- [ ] **Step 2: Verify full workspace compiles**

```bash
cargo build --workspace
# Expected: all crates compile (some may have empty impls)
```

- [ ] **Step 3: Run clippy on all implemented crates**

```bash
cargo clippy -p claudebox-core -p claudebox-witness -p claudebox-meta -p claudebox-migrate -- -D warnings
# Expected: zero warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/
git commit -m "feat: add skeleton crates for full workspace compile"
```
