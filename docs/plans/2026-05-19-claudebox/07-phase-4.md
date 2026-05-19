# ClaudeBox — Phase 4: Kernel Builder + Upgrade + Air-Gap Support

> **Prerequisite:** Phase 3 complete and verified.
> After completing this phase: KernelBuilder cache tests pass, KernelUpgrader segment surgery test passes.

---

### Task 4.1: kernels/build.sh and language profile Dockerfiles

**Files:**
- Create: `kernels/build.sh`
- Create: `kernels/profiles/node.dockerfile`
- Create: `kernels/profiles/python.dockerfile`
- Create: `kernels/profiles/rust.dockerfile`
- Create: `kernels/profiles/go.dockerfile`

- [ ] **Step 1: Create `kernels/build.sh`**

Full script as in Technical Plan §Phase 4:
- Accepts `--lang <lang@version>` (repeatable) and `--output <path>`
- Assembles a multi-stage Dockerfile from per-language fragments in `kernels/profiles/`
- Produces a bootable bzImage at `$OUTPUT`

- [ ] **Step 2: Create per-language Dockerfile fragments**

Each fragment installs the language toolchain into a micro-Linux base image (Alpine or Debian slim).

`kernels/profiles/node.dockerfile` — installs Node.js at specified version
`kernels/profiles/python.dockerfile` — installs Python at specified version
`kernels/profiles/rust.dockerfile` — installs Rust toolchain at specified version
`kernels/profiles/go.dockerfile` — installs Go at specified version

All profiles also install: `openssh-server`, `bash`, `git`, `inotify-tools` (for claudebox-logd).

- [ ] **Step 3: Verify build.sh is executable**

```bash
chmod +x kernels/build.sh
bash -n kernels/build.sh
# Expected: bash syntax check passes (no Docker needed for this check)
```

- [ ] **Step 4: Commit**

```bash
git add kernels/
git commit -m "feat(kernels): add build.sh and per-language Dockerfile fragments"
```

---

### Task 4.2: KernelBuilder with deterministic cache key + cache hit detection

**Files:**
- Create: `crates/claudebox-rvf/src/kernel_builder.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_cache_key_is_sorted_deterministic() {
    let builder = KernelBuilder {
        profiles: vec![
            SingleProfile { lang: Lang::Rust, version: "1.87".into() },
            SingleProfile { lang: Lang::Node, version: "22".into() },
        ],
        project_id: "test".into(),
        ssh_public_key: "ssh-ed25519 AAAA...".into(),
    };
    // Regardless of input order, key must be sorted: node-22_rust-1.87
    assert_eq!(builder.cache_key(), "node-22_rust-1.87");
}

#[test]
fn test_cache_key_single_lang() {
    let builder = KernelBuilder {
        profiles: vec![SingleProfile { lang: Lang::Node, version: "22".into() }],
        project_id: "test".into(),
        ssh_public_key: "ssh-ed25519 AAAA...".into(),
    };
    assert_eq!(builder.cache_key(), "node-22");
}

#[test]
fn test_cached_path_returns_none_when_absent() {
    let builder = KernelBuilder {
        profiles: vec![SingleProfile { lang: Lang::Go, version: "1.22".into() }],
        project_id: "test".into(),
        ssh_public_key: "ssh-ed25519 AAAA...".into(),
    };
    // If cache dir doesn't exist, cached_path returns None
    assert!(builder.cached_path().is_none());
}
```

- [ ] **Step 2: Implement `kernel_builder.rs`**

```rust
pub struct KernelBuilder {
    pub profiles: Vec<SingleProfile>,
    pub project_id: String,
    pub ssh_public_key: String,
}

impl KernelBuilder {
    pub async fn build(&self) -> anyhow::Result<PathBuf> {
        if let Some(cached) = self.cached_path() {
            return Ok(cached);
        }
        // Run kernels/build.sh with all lang profiles
        // Store result at cache_dir()/bzImage
        todo!()
    }

    // Sort profiles by "lang-version" string, join with "_"
    pub fn cache_key(&self) -> String;

    pub fn cached_path(&self) -> Option<PathBuf> {
        let p = self.cache_dir().join("bzImage");
        if p.exists() { Some(p) } else { None }
    }

    pub fn cache_dir(&self) -> PathBuf {
        dirs::home_dir().unwrap()
            .join(".claudebox/kernels")
            .join(self.cache_key())
    }
}
```

- [ ] **Step 3: Run tests and verify pass**

```bash
cargo test -p claudebox-rvf kernel_builder
# Expected: 3/3 pass (build() test is integration-only, not run here)
```

- [ ] **Step 4: Commit**

```bash
git add crates/claudebox-rvf/src/kernel_builder.rs
git commit -m "feat(rvf): implement KernelBuilder with deterministic cache key"
```

---

### Task 4.3: KernelUpgrader — segment surgery

**Files:**
- Create: `crates/claudebox-rvf/src/kernel_upgrade.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_upgrade_result_has_hashes() {
    // Unit test: verify UpgradeResult fields
    let r = UpgradeResult { from_hash: "abc".into(), to_hash: "def".into() };
    assert_ne!(r.from_hash, r.to_hash);
}

// Integration test (requires rvf-cli, Docker) — marked #[ignore]
#[test]
#[ignore = "requires rvf-cli, docker"]
fn test_upgrade_kernel_preserves_meta_seg() {
    // claudebox init with session data in META_SEG
    // KernelUpgrader::upgrade()
    // Verify META_SEG identical to pre-upgrade
    // Verify WITNESS_SEG has KernelUpgrade event
    // Verify kernel_built_at updated
}
```

- [ ] **Step 2: Implement `kernel_upgrade.rs`**

Full `KernelUpgrader` as in Technical Plan §Phase 4:
- `hash_current_kernel()` → SHA3-256 of KERNEL_SEG bytes
- `extract_non_kernel_segments()` → Vec of (seg_name, bytes) excluding KERNEL_SEG
- `build_upgraded_rvf()` → writes new .rvf using non-kernel segs + new kernel
- Uses `InitTransaction` for atomicity
- Appends `WitnessEvent::KernelUpgrade { from_hash, to_hash }`
- Updates `kernel_built_at` in MANIFEST_SEG

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-rvf kernel_upgrade
git add crates/claudebox-rvf/src/kernel_upgrade.rs
git commit -m "feat(rvf): implement KernelUpgrader — segment surgery preserving META/VEC/WITNESS"
```

---

### Task 4.4: KernelImporter — air-gap support

**Files:**
- Create: `crates/claudebox-rvf/src/kernel_import.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_import_from_file_seeds_cache() {
    let dir = tempdir().unwrap();
    let fake_kernel = dir.path().join("bzImage");
    std::fs::write(&fake_kernel, b"fake kernel content").unwrap();

    let cache_dir = dir.path().join("cache");
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(KernelImporter::import_from_file_to(&fake_kernel, &cache_dir)).unwrap();

    assert!(cache_dir.join("bzImage").exists());
    // Verify content matches
    let content = std::fs::read(cache_dir.join("bzImage")).unwrap();
    assert_eq!(content, b"fake kernel content");
}
```

- [ ] **Step 2: Implement `kernel_import.rs`**

```rust
pub struct KernelImporter;

impl KernelImporter {
    // Extract KERNEL_SEG from source_rvf and seed local cache at cache_key path
    pub async fn import_from_rvf(source_rvf: &Path, cache_key: &str) -> anyhow::Result<PathBuf>;

    // Copy raw bzImage to cache — for air-gap environments
    pub async fn import_from_file_to(kernel_path: &Path, cache_dir: &Path) -> anyhow::Result<PathBuf>;
}
```

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-rvf kernel_import
git add crates/claudebox-rvf/src/kernel_import.rs
git commit -m "feat(rvf): implement KernelImporter for air-gap bootstrap"
```

---

### Task 4.5: Kernel staleness warning + `claudebox kernel` CLI integration test

**Files:**
- Create: `crates/claudebox-core/src/staleness.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_staleness_warning_when_over_threshold() {
    // kernel_built_at = 91 days ago, threshold = 90
    let built_at = chrono::Utc::now() - chrono::Duration::days(91);
    let built_str = built_at.to_rfc3339();
    let result = check_kernel_staleness(&built_str, 90);
    assert!(result.is_stale);
    assert!(result.days_old >= 91);
}

#[test]
fn test_staleness_ok_when_under_threshold() {
    let built_at = chrono::Utc::now() - chrono::Duration::days(5);
    let built_str = built_at.to_rfc3339();
    let result = check_kernel_staleness(&built_str, 90);
    assert!(!result.is_stale);
}
```

- [ ] **Step 2: Implement `staleness.rs`**

```rust
pub struct StalenessResult { pub is_stale: bool, pub days_old: i64 }

pub fn check_kernel_staleness(kernel_built_at: &str, threshold_days: u32) -> StalenessResult;
```

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-core staleness
git add crates/claudebox-core/src/staleness.rs crates/claudebox-core/src/lib.rs
git commit -m "feat(core): add kernel staleness check — warns after 90 days"
```
