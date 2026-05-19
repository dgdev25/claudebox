# ClaudeBox — Phase 3: eBPF Network Filter

> **Prerequisite:** Phase 2 complete and verified.
> After completing this phase: eBPF compiler tests pass, squid macOS fallback tests pass.
> **Scope note:** The eBPF filter governs VM egress ONLY — npm install, cargo build, app HTTP.
> Claude Code's Anthropic API calls are host-side and are NEVER filtered here.
> Do NOT add api.anthropic.com to any allowlist.

---

### Task 3.1: eBPF C source — XDP network filter

**Files:**
- Create: `ebpf/network_filter/filter.c`

- [ ] **Step 1: Verify clang is available**

```bash
clang --version | head -1
# Expected: clang version 15.x or higher
```

- [ ] **Step 2: Create `ebpf/network_filter/filter.c`**

Full XDP program as in Technical Plan §Phase 3:
- `BPF_MAP_TYPE_HASH` map of allowed IPv4 addresses (max 256 entries)
- `SEC("xdp")` function `network_filter`:
  1. Parse ethernet header — drop non-IP (`XDP_PASS` for non-IP to avoid breaking other traffic)
  2. Extract dest IPv4
  3. Always `XDP_PASS` for 127.0.0.1 (localhost)
  4. Always `XDP_PASS` for UDP port 53 to dns_server IP
  5. Lookup dest IP in `allowed_ips` map
  6. `XDP_PASS` if found, `XDP_DROP` otherwise

- [ ] **Step 3: Compile the filter to verify it's valid C**

```bash
mkdir -p ebpf/network_filter
clang -O2 -target bpf -c ebpf/network_filter/filter.c -o /tmp/filter_test.o 2>&1
# Expected: compiles without errors
llvm-strip -g /tmp/filter_test.o
file /tmp/filter_test.o
# Expected: ELF output
```

- [ ] **Step 4: Verify ELF magic bytes**

```bash
xxd /tmp/filter_test.o | head -1
# Expected: first 4 bytes are 7f 45 4c 46 (ELF magic)
```

- [ ] **Step 5: Commit**

```bash
git add ebpf/
git commit -m "feat(ebpf): add XDP network filter C source — IPv4 allowlist enforcement"
```

---

### Task 3.2: EbpfCompiler — domain resolution + compile-to-bytes

**Files:**
- Create: `crates/claudebox-ebpf/Cargo.toml`
- Create: `crates/claudebox-ebpf/src/compiler.rs`
- Create: `crates/claudebox-ebpf/src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn test_domain_resolution_returns_ips() {
    let compiler = EbpfCompiler {
        allow_domains: vec!["registry.npmjs.org".into()],
        dns_server: "1.1.1.1".into(),
    };
    let ips = compiler.resolve_allowlist().await.unwrap();
    assert!(!ips.is_empty(), "registry.npmjs.org must resolve to at least one IP");
}

#[test]
#[cfg(target_os = "linux")]
fn test_compiled_bytes_are_elf() {
    let compiler = EbpfCompiler {
        allow_domains: vec!["registry.npmjs.org".into()],
        dns_server: "1.1.1.1".into(),
    };
    let bytes = compiler.compile_to_bytes().unwrap();
    assert_eq!(&bytes[0..4], b"\x7fELF", "output must be ELF binary");
}

#[test]
#[cfg(target_os = "macos")]
fn test_macos_generates_squid_config_not_elf() {
    let generator = SquidConfigGenerator {
        allow_domains: vec!["registry.npmjs.org".into()],
        dns_server: "1.1.1.1".into(),
    };
    let config = generator.generate().unwrap();
    assert!(config.contains("registry.npmjs.org"));
    assert!(config.contains("acl"));
}
```

- [ ] **Step 2: Create `crates/claudebox-ebpf/Cargo.toml`**

```toml
[package]
name = "claudebox-ebpf"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 3: Implement `compiler.rs`**

```rust
pub struct EbpfCompiler {
    pub allow_domains: Vec<String>,
    pub dns_server: String,
}

impl EbpfCompiler {
    // Resolves all domains to IPv4 at init time (static — prevents DNS manipulation)
    pub async fn resolve_allowlist(&self) -> anyhow::Result<Vec<std::net::Ipv4Addr>>;

    // Linux: compiles filter.c with clang, strips with llvm-strip, returns .o path
    pub fn compile(&self) -> anyhow::Result<std::path::PathBuf>;

    // Returns compiled ELF bytes for embedding in EBPF_SEG
    pub fn compile_to_bytes(&self) -> anyhow::Result<Vec<u8>>;
}
```

Compile commands:
```rust
Command::new("clang")
    .args(["-O2", "-target", "bpf", "-c", "ebpf/network_filter/filter.c", "-o", out_path])
    .output()?;
Command::new("llvm-strip").args(["-g", out_path]).output()?;
```

- [ ] **Step 4: Implement `squid.rs`** (macOS fallback)

```rust
pub struct SquidConfigGenerator {
    pub allow_domains: Vec<String>,
    pub dns_server: String,
}

impl SquidConfigGenerator {
    pub fn generate(&self) -> anyhow::Result<String>;
    // Generates squid.conf with ACL entries for each allowed domain
    // http_access allow for each domain, http_access deny all
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p claudebox-ebpf -- --nocapture
# Expected: domain resolution test passes (requires network)
# ELF test passes on Linux, squid test passes on macOS
```

- [ ] **Step 6: Commit**

```bash
git add crates/claudebox-ebpf/
git commit -m "feat(ebpf): implement EbpfCompiler with domain resolution + squid macOS fallback"
```

---

### Task 3.3: Multi-language union allowlist compilation test

**Files:**
- Modify: `crates/claudebox-ebpf/src/compiler.rs` (add test)

- [ ] **Step 1: Write failing test**

```rust
#[test]
#[cfg(target_os = "linux")]
fn test_multi_language_union_allowlist_compiles() {
    // node (2 domains) + rust (3 domains) = 5 unique domains, must compile without error
    let compiler = EbpfCompiler {
        allow_domains: vec![
            "registry.npmjs.org".into(),
            "nodejs.org".into(),
            "crates.io".into(),
            "static.crates.io".into(),
            "index.crates.io".into(),
        ],
        dns_server: "1.1.1.1".into(),
    };
    let bytes = compiler.compile_to_bytes().unwrap();
    assert_eq!(&bytes[0..4], b"\x7fELF");
}
```

- [ ] **Step 2: Run test and verify it passes**

```bash
cargo test -p claudebox-ebpf multi_language
```

- [ ] **Step 3: Commit**

```bash
git add crates/claudebox-ebpf/src/compiler.rs
git commit -m "test(ebpf): add multi-language union allowlist compilation test"
```

---

### Task 3.4: Hypervisor detection + macOS integration

**Files:**
- Create: `crates/claudebox-core/src/hypervisor.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_detect_hypervisor_does_not_panic() {
    // On any platform, detect_hypervisor() must return a value, never panic
    let h = detect_hypervisor();
    // Just verify it returns without panic
    let _ = h;
}
```

- [ ] **Step 2: Implement `hypervisor.rs`**

```rust
#[derive(Debug, PartialEq)]
pub enum Hypervisor { Firecracker, QemuHvf }

pub fn detect_hypervisor() -> Hypervisor {
    if std::path::Path::new("/dev/kvm").exists() {
        Hypervisor::Firecracker
    } else if cfg!(target_os = "macos") {
        Hypervisor::QemuHvf
    } else {
        // Return QemuHvf as fallback but log a warning
        tracing::warn!("No KVM found and not macOS — defaulting to QemuHvf (may fail at runtime)");
        Hypervisor::QemuHvf
    }
}
```

Note: The technical plan's `panic!` in the else branch is removed — fail at runtime (when VM boots) rather than at detection time, per fail-fast-but-informatively principle.

- [ ] **Step 3: Add to claudebox-core lib.rs and run tests**

```bash
cargo test -p claudebox-core hypervisor
```

- [ ] **Step 4: Commit**

```bash
git add crates/claudebox-core/src/hypervisor.rs crates/claudebox-core/src/lib.rs
git commit -m "feat(core): add hypervisor detection for Linux KVM vs macOS HVF"
```
