# ClaudeBox — Phase 2: RVF Appliance Builder + Init Atomicity + Witness Compaction

> **Prerequisite:** Phase 1 complete (`cargo test --workspace` passes).
> After completing this phase: init atomicity tests pass, compaction tests pass.

---

### Task 2.1: InitTransaction RAII struct

**Files:**
- Create: `crates/claudebox-rvf/Cargo.toml`
- Create: `crates/claudebox-rvf/src/transaction.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_commit_renames_tmp_to_final() {
    let dir = tempdir().unwrap();
    let mut tx = InitTransaction::new("myapp", dir.path()).unwrap();
    std::fs::write(tx.tmp_path(), b"content").unwrap();
    tx.commit().unwrap();
    assert!(dir.path().join("myapp.rvf").exists());
    assert!(!dir.path().join("myapp.rvf.tmp").exists());
}

#[test]
fn test_drop_removes_tmp_on_failure() {
    let dir = tempdir().unwrap();
    let tmp_path;
    {
        let tx = InitTransaction::new("myapp", dir.path()).unwrap();
        tmp_path = tx.tmp_path().to_owned();
        std::fs::write(&tmp_path, b"partial").unwrap();
        // tx dropped here without commit
    }
    assert!(!tmp_path.exists());
}

#[test]
fn test_drop_on_panic_cleans_up() {
    let dir = tempdir().unwrap();
    let tmp_path;
    {
        let tx = InitTransaction::new("myapp", dir.path()).unwrap();
        tmp_path = tx.tmp_path().to_owned();
        std::fs::write(&tmp_path, b"partial").unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _tx = tx;
            panic!("simulated panic");
        }));
    }
    assert!(!tmp_path.exists());
}
```

- [ ] **Step 2: Create `crates/claudebox-rvf/Cargo.toml`**

```toml
[package]
name = "claudebox-rvf"
version = "0.1.0"
edition = "2021"

[dependencies]
claudebox-core = { path = "../claudebox-core" }
claudebox-witness = { path = "../claudebox-witness" }
rvf-runtime = { workspace = true }
rvf-crypto = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
ed25519-dalek = { workspace = true }
rand = { workspace = true }
sha3 = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
tokio = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Implement `transaction.rs`** exactly as in Technical Plan §Phase 2

- [ ] **Step 4: Run tests to verify pass**

```bash
cargo test -p claudebox-rvf transaction
# Expected: 3/3 pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-rvf/
git commit -m "feat(rvf): implement InitTransaction RAII — atomic init with panic cleanup"
```

---

### Task 2.2: WitnessCompactor — rolling window + monthly archive

**Files:**
- Create: `crates/claudebox-witness/src/compaction.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_compaction_archives_when_over_limit() {
    // Create 12,000 witness entries in a temp RVF
    // Run WitnessCompactor with max_entries=10,000
    // Verify hot chain has <= 10,000 entries
    // Verify archive file exists
    // Verify compaction appended WitnessCompact event to hot chain
    todo!() // implement after WitnessCompactor struct exists
}

#[test]
fn test_compaction_noop_when_under_limit() {
    let dir = tempdir().unwrap();
    let policy = WitnessPolicy { max_entries: 10_000, retention_days: 30 };
    let compactor = WitnessCompactor {
        rvf_path: dir.path().join("test.rvf"),
        policy,
        archive_dir: dir.path().join("archive"),
    };
    // With 0 entries, compact_if_needed should be a no-op
    // (returns Ok with entries_archived=0, archive_path=None)
}
```

- [ ] **Step 2: Implement `compaction.rs`** from Technical Plan §Phase 2

Include:
- `WitnessCompactor { rvf_path, policy, archive_dir }`
- `compact_if_needed()` → reads entries, partitions by retention, archives to monthly `.rvf`, rewrites hot chain, appends `WitnessCompact` event
- `force_compact()`
- `CompactionResult { entries_archived, entries_kept, archive_path }`

- [ ] **Step 3: Add `pub mod compaction;` to `crates/claudebox-witness/src/lib.rs`**

- [ ] **Step 4: Run tests**

```bash
cargo test -p claudebox-witness
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-witness/src/compaction.rs crates/claudebox-witness/src/lib.rs
git commit -m "feat(witness): implement WitnessCompactor with 30-day rolling window + monthly archive"
```

---

### Task 2.3: ClaudeboxRvfCli wrapper

**Files:**
- Create: `crates/claudebox-rvf/src/rvf_cli.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_rvf_cli_returns_error_for_nonexistent_file() {
    let cli = ClaudeboxRvfCli { binary: "rvf".into() };
    // verify() on a non-existent path returns an error (not a panic)
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(cli.inspect(Path::new("/tmp/does-not-exist.rvf")));
    assert!(result.is_err());
}
```

- [ ] **Step 2: Implement `rvf_cli.rs`**

```rust
pub struct ClaudeboxRvfCli { pub binary: PathBuf }

impl ClaudeboxRvfCli {
    pub async fn derive(&self, source: &Path, output: &Path) -> anyhow::Result<()>;
    pub async fn verify_witness(&self, rvf: &Path) -> anyhow::Result<WitnessVerifyResult>;
    pub async fn inspect(&self, rvf: &Path) -> anyhow::Result<RvfInspectResult>;
    pub async fn compact(&self, rvf: &Path) -> anyhow::Result<()>;
}
```

Each method runs `Command::new(&self.binary).args([subcommand, ...])` and propagates errors with context.

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-rvf rvf_cli
git add crates/claudebox-rvf/src/rvf_cli.rs
git commit -m "feat(rvf): add ClaudeboxRvfCli wrapper for rvf-cli subcommands"
```

---

### Task 2.4: ApplianceBuilder skeleton

**Files:**
- Create: `crates/claudebox-rvf/src/builder.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_builder_new_generates_signing_key() {
    let manifest = test_manifest();
    let builder = ApplianceBuilder::new(manifest).unwrap();
    // Signing key is generated — verifying key must be extractable
    let verifying_key = builder.signing_key.verifying_key();
    assert_eq!(verifying_key.to_bytes().len(), 32);
}
```

- [ ] **Step 2: Implement `builder.rs`** with:

```rust
pub struct ApplianceBuilder {
    pub manifest: ClaudeBoxManifest,
    pub signing_key: ed25519_dalek::SigningKey,
}

impl ApplianceBuilder {
    pub fn new(manifest: ClaudeBoxManifest) -> anyhow::Result<Self>;
    pub fn build_skeleton(&self, output_path: &Path) -> anyhow::Result<()>;
    pub fn embed_kernel(&self, store: &mut RvfStore, kernel_path: &Path) -> anyhow::Result<()>;
    pub fn embed_ebpf(&self, store: &mut RvfStore, ebpf_path: &Path) -> anyhow::Result<()>;
    pub fn write_genesis_witness(&self, store: &mut RvfStore) -> anyhow::Result<()>;
    pub fn verify(&self, rvf_path: &Path) -> anyhow::Result<()>;
}
```

`build_skeleton` uses `InitTransaction` internally.

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-rvf builder
git add crates/claudebox-rvf/src/builder.rs
git commit -m "feat(rvf): add ApplianceBuilder skeleton with Ed25519 key generation"
```

---

### Task 2.5: lib.rs module wiring for claudebox-rvf

**Files:**
- Modify: `crates/claudebox-rvf/src/lib.rs`

- [ ] **Step 1: Add all module declarations**

```rust
pub mod builder;
pub mod transaction;
pub mod rvf_cli;
pub mod kernel_builder;
pub mod kernel_upgrade;
pub mod kernel_import;
```

Create empty stub files for `kernel_builder.rs`, `kernel_upgrade.rs`, `kernel_import.rs` (full impl in Phase 4).

- [ ] **Step 2: Verify workspace compiles clean**

```bash
cargo build --workspace
cargo clippy -p claudebox-rvf -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/claudebox-rvf/src/
git commit -m "feat(rvf): wire up module declarations and add kernel_* stubs"
```

---

### Task 2.6: claudebox-core `claudebox init` logic

**Files:**
- Create: `crates/claudebox-core/src/init.rs`

- [ ] **Step 1: Write integration-level test (unit scope)**

```rust
#[test]
fn test_init_manifest_built_correctly_for_single_lang() {
    let opts = InitOptions {
        name: "myapp".into(),
        lang: vec!["node@22".into()],
        allow: vec![],
        kernel_from: None,
    };
    let manifest = build_manifest_from_opts(&opts).unwrap();
    assert_eq!(manifest.project_name, "myapp");
    assert_eq!(manifest.version, 1);
    match &manifest.language {
        LanguageProfile::Single(p) => assert!(matches!(p.lang, Lang::Node)),
        _ => panic!("Expected Single"),
    }
    assert!(manifest.network.allow_domains.contains(&"registry.npmjs.org".to_string()));
}
```

- [ ] **Step 2: Implement `init.rs`**

```rust
pub struct InitOptions {
    pub name: String,
    pub lang: Vec<String>,     // e.g. ["node@22", "rust@1.87"]
    pub allow: Vec<String>,    // additional domains
    pub kernel_from: Option<PathBuf>,
}

pub fn build_manifest_from_opts(opts: &InitOptions) -> anyhow::Result<ClaudeBoxManifest>;
pub async fn run_init(opts: InitOptions, output_dir: &Path) -> anyhow::Result<()>;
```

`run_init` orchestrates the full init flow from pseudocode: parse lang(s), build manifest, start InitTransaction, build/import kernel, compile eBPF, build appliance skeleton, commit.

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-core init
git add crates/claudebox-core/src/init.rs
git commit -m "feat(core): add init logic — manifest builder and run_init orchestrator"
```

---

### Task 2.7: Add `pub mod init;` to claudebox-core + full phase verification

- [ ] **Step 1: Update `crates/claudebox-core/src/lib.rs`**

```rust
pub mod manifest;
pub mod preflight;
pub mod shell_bridge;
pub mod init;
```

- [ ] **Step 2: Run full test suite for phase**

```bash
cargo test --workspace
# Expected: all non-ignored tests pass
```

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
# Expected: zero warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/claudebox-core/src/lib.rs
git commit -m "feat(core): wire init module; phase 2 complete"
```
