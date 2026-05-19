# ClaudeBox — Phase 8: CLI Subcommands

> **Prerequisite:** Phase 7 complete and verified.
> After completing this phase: all subcommands parse correctly, `claudebox --help` works.

---

### Task 8.1: claudebox-cli — Clap CLI structure

**Files:**
- Create: `crates/claudebox-cli/Cargo.toml`
- Create: `crates/claudebox-cli/src/main.rs`

- [ ] **Step 1: Write failing tests — clap subcommand parsing**

```rust
#[test]
fn test_init_subcommand_parses_single_lang() {
    let cli = Cli::try_parse_from(["claudebox", "init", "myapp", "--lang", "node@22"]).unwrap();
    match cli.command {
        Commands::Init { name, lang, .. } => {
            assert_eq!(name, "myapp");
            assert_eq!(lang, vec!["node@22"]);
        }
        _ => panic!("Expected Init"),
    }
}

#[test]
fn test_init_subcommand_parses_multi_lang() {
    let cli = Cli::try_parse_from(["claudebox", "init", "polyglot",
        "--lang", "node@22,rust@1.87"]).unwrap();
    match cli.command {
        Commands::Init { lang, .. } => {
            assert_eq!(lang.len(), 2);
            assert!(lang.contains(&"node@22".to_string()));
            assert!(lang.contains(&"rust@1.87".to_string()));
        }
        _ => panic!("Expected Init"),
    }
}

#[test]
fn test_start_subcommand_default_workspace() {
    let cli = Cli::try_parse_from(["claudebox", "start", "myapp.rvf"]).unwrap();
    match cli.command {
        Commands::Start { workspace, .. } => {
            assert_eq!(workspace, PathBuf::from("."));
        }
        _ => panic!("Expected Start"),
    }
}

#[test]
fn test_audit_subcommand_with_archive_flag() {
    let cli = Cli::try_parse_from(["claudebox", "audit", "myapp.rvf",
        "--archive", "2026-04"]).unwrap();
    match cli.command {
        Commands::Audit { archive, .. } => {
            assert_eq!(archive.unwrap(), "2026-04");
        }
        _ => panic!("Expected Audit"),
    }
}
```

- [ ] **Step 2: Create `crates/claudebox-cli/Cargo.toml`**

```toml
[package]
name = "claudebox-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "claudebox"
path = "src/main.rs"

[dependencies]
claudebox-core = { path = "../claudebox-core" }
claudebox-rvf = { path = "../claudebox-rvf" }
claudebox-firecracker = { path = "../claudebox-firecracker" }
claudebox-vec = { path = "../claudebox-vec" }
claudebox-witness = { path = "../claudebox-witness" }
claudebox-meta = { path = "../claudebox-meta" }
claudebox-migrate = { path = "../claudebox-migrate" }
clap = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
```

- [ ] **Step 3: Implement `main.rs`** with full Clap CLI

Full `Cli`, `Commands`, `SnapshotAction`, `KernelAction` enums exactly as in Technical Plan §Phase 8, including ALL subcommands:
- `init`, `start`, `stop`, `logs`, `status`, `branch`, `rollback`, `audit`, `snapshot`, `upgrade-kernel`, `kernel`, `migrate`, `compact`, `update-allowlist`, `destroy`

Dispatch each subcommand to the corresponding `run_*` function in the relevant crate.

- [ ] **Step 4: Run tests to verify parsing**

```bash
cargo test -p claudebox-cli
# Expected: 4/4 parsing tests pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-cli/
git commit -m "feat(cli): implement full Clap CLI with all 15 subcommands"
```

---

### Task 8.2: `claudebox status` output formatter

**Files:**
- Create: `crates/claudebox-core/src/status.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_status_output_contains_required_fields() {
    let info = ProjectStatus {
        project_name: "myproject".into(),
        rvf_path: PathBuf::from("myproject.rvf"),
        rvf_size_mb: 245,
        vm_status: VmStatusDisplay::Running { pid: 18432, ssh_port: 2222, mcp_port: 7878 },
        language: "node@22".into(),
        kernel_age_days: 12,
        kernel_stale: false,
        witness_hot_entries: 1247,
        witness_archived_months: 2,
        vec_chunks: 4821,
        vec_files: 312,
        vec_tombstoned: 3,
        schema_version: 1,
    };
    let output = format_status(&info);
    assert!(output.contains("myproject"));
    assert!(output.contains("245 MB"));
    assert!(output.contains("18432"));
    assert!(output.contains("1,247"));
    assert!(output.contains("312 files"));
    assert!(output.contains("tombstoned"));
}

#[test]
fn test_status_shows_kernel_staleness_warning() {
    let info = ProjectStatus { kernel_stale: true, kernel_age_days: 94, ..default_status() };
    let output = format_status(&info);
    assert!(output.contains("⚠") || output.contains("warning") || output.contains("stale"));
    assert!(output.contains("upgrade-kernel"));
}
```

- [ ] **Step 2: Implement `status.rs`**

`ProjectStatus` struct + `format_status()` → returns the multi-line status string matching Technical Plan §Phase 8 example output.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-core status
git add crates/claudebox-core/src/status.rs
git commit -m "feat(core): implement claudebox status output formatter"
```

---

### Task 8.3: `claudebox audit` output formatter + chain verifier

**Files:**
- Create: `crates/claudebox-witness/src/audit.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_audit_table_output_format() {
    let entries = vec![
        WitnessEntry { seq: 0, ts_nanos: 0, event: WitnessEvent::Boot { project_id: "x".into() },
            payload_hash: [0u8;32], prev_hash: [0u8;32], signature: [0u8;64] },
    ];
    let output = format_audit_table(&entries);
    assert!(output.contains("Chain integrity"));
    assert!(output.contains("BOOT"));
}

#[test]
fn test_chain_integrity_valid() {
    // Genesis entry should always verify against itself
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    let genesis = WitnessWriter::create_genesis(&signing_key,
        WitnessEvent::Boot { project_id: "test".into() }).unwrap();
    let result = verify_chain(&[genesis]);
    assert!(result.is_valid);
    assert_eq!(result.broken_at_seq, None);
}

#[test]
fn test_chain_integrity_detects_tamper() {
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    let mut genesis = WitnessWriter::create_genesis(&signing_key,
        WitnessEvent::Boot { project_id: "test".into() }).unwrap();
    // Tamper with payload_hash
    genesis.payload_hash[0] ^= 0xFF;
    let result = verify_chain(&[genesis]);
    assert!(!result.is_valid);
}
```

- [ ] **Step 2: Implement `audit.rs`**

```rust
pub struct ChainVerifyResult { pub is_valid: bool, pub broken_at_seq: Option<u64> }

pub fn verify_chain(entries: &[WitnessEntry]) -> ChainVerifyResult;
pub fn format_audit_table(entries: &[WitnessEntry]) -> String;
pub fn format_audit_json(entries: &[WitnessEntry]) -> String;
```

`verify_chain`: for each entry, recompute SHA3-256 of event payload, verify it matches `payload_hash`, verify `prev_hash` matches previous entry's `payload_hash`, verify Ed25519 signature.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-witness audit
git add crates/claudebox-witness/src/audit.rs
git commit -m "feat(witness): add audit formatter and chain integrity verifier"
```

---

### Task 8.4: `claudebox update-allowlist` — eBPF recompile

**Files:**
- Create: `crates/claudebox-core/src/allowlist.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_add_domain_to_allowlist() {
    let mut policy = NetworkPolicy {
        allow_domains: vec!["registry.npmjs.org".into()],
        allow_localhost: true,
        dns_server: "1.1.1.1".into(),
    };
    add_domain_to_policy(&mut policy, "example.com");
    assert!(policy.allow_domains.contains(&"example.com".to_string()));
}

#[test]
fn test_remove_domain_from_allowlist() {
    let mut policy = NetworkPolicy {
        allow_domains: vec!["registry.npmjs.org".into(), "nodejs.org".into()],
        allow_localhost: true,
        dns_server: "1.1.1.1".into(),
    };
    remove_domain_from_policy(&mut policy, "nodejs.org");
    assert!(!policy.allow_domains.contains(&"nodejs.org".to_string()));
    assert_eq!(policy.allow_domains.len(), 1);
}
```

- [ ] **Step 2: Implement `allowlist.rs`**

```rust
pub fn add_domain_to_policy(policy: &mut NetworkPolicy, domain: &str);
pub fn remove_domain_from_policy(policy: &mut NetworkPolicy, domain: &str);

pub async fn run_update_allowlist(
    rvf_path: &Path,
    add: Vec<String>,
    remove: Vec<String>,
) -> anyhow::Result<()>;
```

`run_update_allowlist`:
1. Read MANIFEST_SEG
2. Mutate `network.allow_domains`
3. Recompile eBPF (or regenerate squid config on macOS)
4. Update EBPF_SEG in .rvf via InitTransaction
5. Update MANIFEST_SEG
6. Append `NetworkRequest` or `Branch` event to WITNESS_SEG (use a new variant if needed)

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-core allowlist
git add crates/claudebox-core/src/allowlist.rs
git commit -m "feat(core): implement update-allowlist — eBPF recompile on domain add/remove"
```

---

### Task 8.5: Full CLI integration test + `cargo build --release`

**Files:** No new files — verification gate

- [ ] **Step 1: Run full test suite**

```bash
cargo test --workspace -- --skip ignored
# Expected: all non-ignored tests pass
```

- [ ] **Step 2: Build release binary**

```bash
cargo build --workspace --release
# Expected: zero errors, produces target/release/claudebox
```

- [ ] **Step 3: Verify `--help` output**

```bash
./target/release/claudebox --help
# Expected: usage text showing all 15 subcommands

./target/release/claudebox init --help
./target/release/claudebox start --help
./target/release/claudebox audit --help
```

- [ ] **Step 4: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
# Expected: zero warnings
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(cli): phase 8 complete — full CLI verified, release build passes"
```
