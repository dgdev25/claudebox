# ClaudeBox — Technical Implementation Plan

**Version:** 2.0
**Stack:** Rust (primary), Firecracker, RuVector/RVF, eBPF
**Date:** 2026-05-19

---

## CLAUDE CODE INSTRUCTIONS

You are building **ClaudeBox** — a per-project isolated execution environment for Claude Code workflows.
Read this entire document before writing any code. The build order in Section 4 is strict — do not skip phases.
All code is written in Rust unless explicitly stated otherwise.
Use `cargo workspace` layout. All crates live under `crates/`.
Run `cargo test` after completing each phase. Do not proceed to the next phase if tests fail.

### How Claude Code Integrates With ClaudeBox

Claude Code is a **host-side process**. It runs on the developer's machine, calls the Anthropic API directly from the host, and executes tool calls (bash, file read/write) via its configured shell. ClaudeBox does **not** run Claude Code inside the VM.

The integration model:

```
HOST MACHINE
├── Claude Code CLI  ←→  api.anthropic.com  (unrestricted, host network)
│     └── bash tool calls → SSH → Firecracker VM shell
│     └── file tool calls → /workspace (host directory)
│     └── mcp tool calls  → SSH tunnel → :7878 (ClaudeBox MCP server in VM)
│
FIRECRACKER MICROVM (boots from .rvf)
└── /workspace (virtio-fs mount of host directory — live, bidirectional)
    language toolchain, dev server, test runner
    eBPF XDP filter (governs VM egress only — NOT Claude Code's API calls)
```

`claudebox start` boots the VM and writes `.claude/settings.json` into the workspace, redirecting Claude Code's bash tool through an SSH wrapper into the VM. Claude Code's Anthropic API calls are **not affected** by the eBPF filter — they remain on the host network. The eBPF filter governs only what agent-executed code (npm install, cargo build, app HTTP calls) can reach from inside the VM.

Claude Code is launched interactively (`claude`). The `-p`/`--print` non-interactive flag is **not used** by ClaudeBox.

---

## 1. Repository Structure

```
claudebox/
├── Cargo.toml                        # workspace root
├── crates/
│   ├── claudebox-cli/                # main binary — the `claudebox` command
│   ├── claudebox-core/               # core logic: init, start, branch, audit, preflight
│   ├── claudebox-rvf/                # RVF appliance builder (wraps rvf-runtime)
│   ├── claudebox-firecracker/        # Firecracker microVM lifecycle management
│   ├── claudebox-ebpf/               # eBPF network filter compilation + embedding
│   ├── claudebox-vec/                # VEC_SEG indexing pipeline (codebase embeddings)
│   ├── claudebox-witness/            # WITNESS_SEG writer, verifier, compaction
│   ├── claudebox-meta/               # META_SEG session state serialisation + hooks
│   ├── claudebox-migrate/            # RVF format version migration transformers
│   └── claudebox-logd/               # vsock log forwarder daemon (runs inside VM)
├── ebpf/
│   └── network_filter/               # eBPF C source (compiled to bytecode, embedded)
├── kernels/
│   ├── build.sh                      # Docker-based micro-Linux kernel build script
│   └── profiles/                     # per-language Dockerfile fragments
├── examples/
│   └── node_project.rvf              # pre-built example appliance
└── tests/
    └── integration/                  # end-to-end tests (all #[ignore] by default)
```

---

## 2. Dependencies

### 2.1 Cargo.toml (workspace root)

```toml
[workspace]
resolver = "2"
members = [
    "crates/claudebox-cli",
    "crates/claudebox-core",
    "crates/claudebox-rvf",
    "crates/claudebox-firecracker",
    "crates/claudebox-ebpf",
    "crates/claudebox-vec",
    "crates/claudebox-witness",
    "crates/claudebox-meta",
    "crates/claudebox-migrate",
    "crates/claudebox-logd",
]

[workspace.dependencies]
# RVF / RuVector
rvf-runtime = "0.2"
rvf-crypto = "0.2"
rvf-types = "0.1"

# Async runtime
tokio = { version = "1", features = ["full"] }

# CLI
clap = { version = "4", features = ["derive"] }

# Serialisation
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Crypto
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand = "0.8"
sha3 = "0.10"

# Embeddings
fastembed = "3"          # provides all-MiniLM-L6-v2 locally, no API call

# Errors
anyhow = "1"
thiserror = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Time
chrono = { version = "0.4", features = ["serde"] }

# File watching (incremental VEC_SEG updates)
notify = "6"

# Port detection (MCP port conflict resolution)
port-check = "0.2"

# UUID generation
uuid = { version = "1", features = ["v4"] }

# Process management
tokio-process = "1"
```

### 2.2 External System Dependencies

Claude Code must verify these exist before building. If missing, emit a clear error with install instructions.

```
firecracker       # >= 1.7.0   — microVM runtime
virtiofsd         # for workspace virtio-fs bind mount
kvm               # host kernel module — check /dev/kvm exists and is readable
docker            # for kernel build only (build.sh) — >= 24.0
rvf-cli           # cargo install rvf-cli (from ruvnet/RuVector)
clang             # >= 15 — for eBPF compilation
llvm-strip        # for stripping eBPF objects
ssh               # OpenSSH client — for shell bridge
```

```rust
// crates/claudebox-core/src/preflight.rs
pub fn check_dependencies() -> anyhow::Result<()> {
    // Check /dev/kvm exists and is readable (skip check on macOS, use QEMU path)
    // Check firecracker --version >= 1.7.0
    // Check virtiofsd --version
    // Check clang --version >= 15
    // Check rvf-cli --version
    // Check ssh -V
    // Return structured error per missing dep with install instructions
}
```

---

## 3. Data Structures

### 3.1 ClaudeBox Manifest (written into MANIFEST_SEG)

```rust
// crates/claudebox-core/src/manifest.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClaudeBoxManifest {
    pub version: u8,                        // manifest schema version, currently 1
    pub project_id: String,                 // UUID v4
    pub project_name: String,
    pub language: LanguageProfile,
    pub created_at: String,                 // ISO8601
    pub kernel_built_at: String,            // ISO8601 — used for staleness warning
    pub network: NetworkPolicy,
    pub resources: ResourceLimits,
    pub kernel: KernelConfig,
    pub witness: WitnessPolicy,             // retention policy
}

// REMEDIATION BS-3: Multi-language support.
// LanguageProfile supports single or multiple language targets.
// Multi variant produces a union of all per-language network policies.
// The kernel build script receives all profiles and installs all toolchains.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum LanguageProfile {
    Single(SingleProfile),
    Multi(Vec<SingleProfile>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SingleProfile {
    pub lang: Lang,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Lang { Node, Python, Rust, Go }

impl LanguageProfile {
    // Returns all profiles as a flat vec regardless of Single/Multi
    pub fn profiles(&self) -> Vec<&SingleProfile> {
        match self {
            LanguageProfile::Single(p) => vec![p],
            LanguageProfile::Multi(ps) => ps.iter().collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkPolicy {
    pub allow_domains: Vec<String>,         // FQDN allowlist — deduplicated union
    pub allow_localhost: bool,              // allow 127.0.0.1 (for local dev servers)
    pub dns_server: String,                 // default: "1.1.1.1"
}

impl NetworkPolicy {
    // Builds a deduplicated union allowlist across all language profiles
    pub fn for_profiles(profiles: &[&SingleProfile]) -> Self {
        let mut domains: Vec<String> = profiles.iter()
            .flat_map(|p| Self::domains_for_lang(&p.lang))
            .collect();
        domains.sort();
        domains.dedup();
        NetworkPolicy { allow_domains: domains, allow_localhost: true, dns_server: "1.1.1.1".into() }
    }

    fn domains_for_lang(lang: &Lang) -> Vec<String> {
        match lang {
            Lang::Node   => vec!["registry.npmjs.org".into(), "nodejs.org".into()],
            Lang::Python => vec!["pypi.org".into(), "files.pythonhosted.org".into()],
            Lang::Rust   => vec!["crates.io".into(), "static.crates.io".into(), "index.crates.io".into()],
            Lang::Go     => vec!["proxy.golang.org".into(), "sum.golang.org".into()],
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResourceLimits {
    pub vcpus: u8,          // default: 2
    pub memory_mb: u32,     // default: 4096
    pub disk_gb: u32,       // default: 20
    pub network_mbps: u32,  // default: 100
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self { vcpus: 2, memory_mb: 4096, disk_gb: 20, network_mbps: 100 }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KernelConfig {
    pub arch: String,       // "x86_64" | "aarch64"
    pub ssh_port: u16,      // default: 2222
    pub mcp_port: u16,      // default: 7878 — REMEDIATION BS-9: not 8080 (avoids dev server conflict)
}

// REMEDIATION BS-1: WITNESS_SEG retention policy embedded in manifest.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WitnessPolicy {
    pub max_entries: u32,       // default: 10_000 (~640 KB at 64B per entry)
    pub retention_days: u32,    // default: 30 — older entries are archived to monthly .rvf files
}

impl Default for WitnessPolicy {
    fn default() -> Self { Self { max_entries: 10_000, retention_days: 30 } }
}
```

### 3.2 Session State (written into META_SEG)

```rust
// crates/claudebox-meta/src/lib.rs

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub last_boot: Option<String>,
    pub working_dir: String,
    pub open_files: Vec<String>,
    pub task_context: String,               // Claude's current task description
    pub scratchpad: String,                 // Claude's free-form project notes
    pub history: Vec<HistoryEntry>,
    pub installed_packages: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts: String,
    pub kind: HistoryEntryKind,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum HistoryEntryKind {
    Command, FileWrite, FileDelete, PackageInstall, Note,
}

impl SessionState {
    pub fn to_claude_prompt_prefix(&self) -> String {
        format!(
            "## ClaudeBox Session Context\n\
             Last session: {last}\n\
             Working directory: {wd}\n\
             Current task: {task}\n\
             Notes from last session:\n{notes}\n\
             Recent history (last 20 entries):\n{history}",
            last    = self.last_boot.as_deref().unwrap_or("first boot"),
            wd      = self.working_dir,
            task    = self.task_context,
            notes   = self.scratchpad,
            history = self.history.iter().rev().take(20)
                          .map(|e| format!("  - [{}] {}", e.ts, e.value))
                          .collect::<Vec<_>>().join("\n"),
        )
    }
}
```

### 3.3 Witness Entry (WITNESS_SEG record)

```rust
// crates/claudebox-witness/src/lib.rs

#[derive(Debug, Serialize, Deserialize)]
pub struct WitnessEntry {
    pub seq: u64,                   // monotonic sequence number
    pub ts_nanos: u128,             // nanosecond timestamp
    pub event: WitnessEvent,
    pub payload_hash: [u8; 32],    // SHA3-256 of event payload
    pub prev_hash: [u8; 32],       // hash of previous entry (genesis = [0u8;32])
    pub signature: [u8; 64],       // Ed25519 over (seq + ts + payload_hash + prev_hash)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WitnessEvent {
    Boot            { project_id: String },
    Shutdown        { reason: String },
    Command         { cmd: String, exit_code: i32 },
    FileWrite       { path: String, size_bytes: u64 },
    FileDelete      { path: String },
    NetworkRequest  { domain: String, method: String, status: u16 },
    PackageInstall  { name: String, version: String, registry: String },
    Snapshot        { name: String },
    Branch          { child_name: String },
    Rollback        { target: String },
    // REMEDIATION BS-2: kernel upgrade tracking
    KernelUpgrade   { from_hash: String, to_hash: String },
    // REMEDIATION BS-1: witness compaction tracking
    WitnessCompact  { entries_archived: u32, archive_path: String },
    // REMEDIATION BS-5: stale VEC entry tracking
    VecReconcile    { files_removed: u32 },
    // REMEDIATION BS-7: format migration tracking
    FormatMigrate   { from_version: u8, to_version: u8 },
}
```

---

## 4. Build Order — Follow This Exactly

Complete each phase fully with passing tests before starting the next.

---

### Phase 0: SSH Shell Bridge

**Goal:** Claude Code's bash tool executes commands inside the Firecracker VM transparently via SSH. This is the core integration mechanism — nothing else works without it.

**File:** `~/.claudebox/bin/claudebox-shell`

```bash
#!/usr/bin/env bash
# ClaudeBox SSH shell bridge
# Claude Code's bash tool uses this as its executor via .claude/settings.json
# All bash tool calls SSH into the running Firecracker VM transparently

exec ssh \
  -i "${CLAUDEBOX_KEY_PATH}" \
  -p "${CLAUDEBOX_SSH_PORT}" \
  -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null \
  -o ConnectTimeout=5 \
  claude@127.0.0.1 "$@"
```

**File:** `crates/claudebox-core/src/shell_bridge.rs`

```rust
pub struct ShellBridge {
    pub project_id: String,
    pub ssh_port: u16,
    pub key_path: PathBuf,
    pub mcp_port: u16,
}

impl ShellBridge {
    // Writes ~/.claudebox/bin/claudebox-shell with permissions 0o755
    pub fn install_shell_script(&self) -> anyhow::Result<()>;

    // Writes .claude/settings.json into workspace directory.
    // Redirects Claude Code's bash tool through the SSH bridge.
    pub fn write_claude_settings(&self, workspace: &Path) -> anyhow::Result<()>;

    // Writes .claude/mcp.json into workspace directory.
    // Configures the ClaudeBox MCP server tools for Claude Code.
    pub fn write_mcp_config(&self, workspace: &Path, actual_mcp_port: u16) -> anyhow::Result<()>;
}
```

**`.claude/settings.json`** written by `write_claude_settings`:
```json
{
  "shell": "/home/user/.claudebox/bin/claudebox-shell",
  "env": {
    "CLAUDEBOX_KEY_PATH": "/home/user/.claudebox/keys/<project-id>.key",
    "CLAUDEBOX_SSH_PORT": "2222"
  }
}
```

**`.claude/mcp.json`** written by `write_mcp_config`:
```json
{
  "mcpServers": {
    "claudebox": {
      "command": "ssh",
      "args": [
        "-i", "/home/user/.claudebox/keys/<project-id>.key",
        "-p", "2222",
        "-L", "7878:localhost:7878",
        "-o", "StrictHostKeyChecking=no",
        "claude@127.0.0.1",
        "claudebox-mcp"
      ],
      "transport": "stdio"
    }
  }
}
```

Note: `7878` in the MCP config is replaced at runtime with the actual port detected in Phase 7. `write_mcp_config` accepts `actual_mcp_port` as a parameter.

**Tests:**
- `install_shell_script` produces a file with mode `0o755`
- `write_claude_settings` produces valid JSON with correct port and key path substituted
- `write_mcp_config` produces valid JSON with the passed `actual_mcp_port`
- Running the shell script with `--version` via a real SSH target produces non-empty output (integration, `#[ignore]`)

---

### Phase 1: Foundation — Core Types + Manifest

**Goal:** Workspace compiles, all data structures defined, manifest serialises/deserialises correctly.

**Tasks:**
1. Create workspace `Cargo.toml` with all members listed in Section 2.1
2. Create `crates/claudebox-core/` with `manifest.rs`, `preflight.rs`, `shell_bridge.rs`
3. Create `crates/claudebox-meta/` with session state types
4. Create `crates/claudebox-witness/` with all witness entry types including all new event variants
5. Create `crates/claudebox-migrate/` with `SegmentMigrator` trait stub (full impl in Phase 11)

**Tests:**
- Manifest serialisation round-trip: `ClaudeBoxManifest` → JSON → `ClaudeBoxManifest`, all fields intact
- `LanguageProfile::Multi([node@22, rust@1.87])` union policy produces 5 unique domains, no duplicates
- `LanguageProfile::Single(node@22)` produces 2 domains
- `WitnessPolicy::default()` has `max_entries = 10_000` and `retention_days = 30`
- `KernelConfig` default `mcp_port` is `7878`, not `8080`
- `cargo clippy -p claudebox-core` produces zero warnings

---

### Phase 2: RVF Appliance Builder + Init Atomicity + Witness Compaction

**Goal:** Creates a valid `.rvf` skeleton. Init is atomic — any failure leaves no artefacts on disk. WITNESS_SEG compaction is implemented.

**File:** `crates/claudebox-rvf/src/builder.rs`

```rust
pub struct ApplianceBuilder {
    pub manifest: ClaudeBoxManifest,
    pub signing_key: ed25519_dalek::SigningKey,
}

impl ApplianceBuilder {
    pub fn new(manifest: ClaudeBoxManifest) -> anyhow::Result<Self>;

    // Creates the .rvf skeleton with all segments except KERNEL_SEG and EBPF_SEG.
    // Those are added by embed_kernel() and embed_ebpf() after compilation.
    // Uses InitTransaction internally — output_path is the final destination.
    pub fn build_skeleton(&self, output_path: &Path) -> anyhow::Result<()>;

    pub fn embed_kernel(&self, store: &mut RvfStore, kernel_path: &Path) -> anyhow::Result<()>;
    pub fn embed_ebpf(&self, store: &mut RvfStore, ebpf_path: &Path) -> anyhow::Result<()>;
    pub fn write_genesis_witness(&self, store: &mut RvfStore) -> anyhow::Result<()>;
    pub fn verify(&self, rvf_path: &Path) -> anyhow::Result<()>;
}
```

**REMEDIATION BS-10: Init atomicity — `InitTransaction` RAII struct**

All init file writes go through `InitTransaction`. The final `.rvf` only appears on disk after a successful `commit()`. On any failure — including panics — `Drop` cleans up temp files automatically.

```rust
// crates/claudebox-rvf/src/transaction.rs

pub struct InitTransaction {
    tmp_path: PathBuf,          // <name>.rvf.tmp — written during init
    final_path: PathBuf,        // <name>.rvf — only exists after commit()
    cleanup_paths: Vec<PathBuf>, // kernel extracts, eBPF .o files, etc.
    committed: bool,
}

impl InitTransaction {
    pub fn new(name: &str, dir: &Path) -> anyhow::Result<Self>;

    pub fn tmp_path(&self) -> &Path { &self.tmp_path }

    pub fn register_cleanup(&mut self, path: PathBuf);

    // Called only on explicit success. Atomically renames .tmp → final.
    pub fn commit(mut self) -> anyhow::Result<()> {
        std::fs::rename(&self.tmp_path, &self.final_path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for InitTransaction {
    fn drop(&mut self) {
        if !self.committed {
            // Best-effort cleanup on failure or panic
            let _ = std::fs::remove_file(&self.tmp_path);
            for path in &self.cleanup_paths {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
```

**REMEDIATION BS-1: WITNESS_SEG compaction**

```rust
// crates/claudebox-witness/src/compaction.rs

pub struct WitnessCompactor {
    pub rvf_path: PathBuf,
    pub policy: WitnessPolicy,
    pub archive_dir: PathBuf,   // ~/.claudebox/archive/<project-id>/
}

impl WitnessCompactor {
    // Called automatically on boot if entry count or age threshold is exceeded.
    // 1. Read all WITNESS_SEG entries
    // 2. Partition into: keep (within retention window) + archive (older or over limit)
    // 3. Write archived entries to a COW-derived .rvf in archive_dir
    //    named: witness-<YYYY-MM>.rvf (one file per calendar month)
    // 4. Rewrite WITNESS_SEG with only kept entries, maintaining chain integrity
    // 5. Append WitnessCompact event to the new chain
    pub async fn compact_if_needed(&self) -> anyhow::Result<CompactionResult>;

    pub async fn force_compact(&self) -> anyhow::Result<CompactionResult>;
}

pub struct CompactionResult {
    pub entries_archived: u32,
    pub entries_kept: u32,
    pub archive_path: Option<PathBuf>,  // None if nothing needed archiving
}
```

`claudebox audit --archive 2026-04` reads monthly archive files directly — they are valid `.rvf` files with intact WITNESS_SEG chains, openable with `rvf_runtime::RvfStore::open()`.

**Tests:**
- `InitTransaction::drop` removes `.rvf.tmp` on simulated failure (verify file absent after drop)
- `InitTransaction::commit` renames correctly; `.rvf.tmp` absent, `<name>.rvf` present
- `InitTransaction::drop` on panic cleans up (use `std::panic::catch_unwind` in test)
- `WitnessCompactor` with 12,000 entries archives oldest 2,000 when `max_entries = 10,000`
- Archive file is a valid `.rvf` openable with `RvfStore::open()`
- Compaction appends `WitnessCompact` event to retained chain
- Hot chain after compaction passes `rvf verify-witness`

---

### Phase 3: eBPF Network Filter

**Goal:** Compile eBPF XDP socket filter from C source enforcing the domain allowlist. Embed bytecode in EBPF_SEG.

**Scope:** The eBPF filter governs egress from **inside the Firecracker VM only** — npm installs, cargo fetches, app HTTP calls. Claude Code's Anthropic API calls are made from the host and are entirely outside this filter's scope. Do not add `api.anthropic.com` to any allowlist.

**File:** `ebpf/network_filter/filter.c`

```c
// XDP program enforcing IPv4 egress allowlist.
// Allowlist IPs are resolved at appliance init time and stored in a BPF_MAP_TYPE_HASH.
// DNS traffic (port 53) to the configured resolver is always allowed.
// Localhost (127.0.0.1) is always allowed.
// All other egress is XDP_DROP.

#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/tcp.h>
#include <linux/udp.h>

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 256);
    __type(key, __u32);     // IPv4 address (network byte order)
    __type(value, __u8);    // 1 = allowed
} allowed_ips SEC(".maps");

SEC("xdp")
int network_filter(struct xdp_md *ctx) {
    // 1. Parse ethernet header — drop non-IP frames
    // 2. Extract dest IPv4 address
    // 3. Always XDP_PASS: 127.0.0.1 (localhost)
    // 4. Always XDP_PASS: UDP port 53 to dns_server IP
    // 5. Lookup dest IP in allowed_ips map
    // 6. XDP_PASS if found, XDP_DROP otherwise
}
```

**File:** `crates/claudebox-ebpf/src/compiler.rs`

```rust
pub struct EbpfCompiler {
    pub allow_domains: Vec<String>,
    pub dns_server: String,
}

impl EbpfCompiler {
    // Resolves all domains to IPv4 at init time.
    // IPs are baked into the BPF map — static by design.
    // Static resolution prevents DNS manipulation bypasses from inside the VM.
    pub async fn resolve_allowlist(&self) -> anyhow::Result<Vec<std::net::Ipv4Addr>>;

    // Compiles filter.c with clang, strips with llvm-strip, returns .o path
    pub fn compile(&self) -> anyhow::Result<PathBuf>;

    // Returns compiled .o bytes for embedding in EBPF_SEG
    pub fn compile_to_bytes(&self) -> anyhow::Result<Vec<u8>>;
}
```

Compile commands:
```bash
clang -O2 -target bpf -c ebpf/network_filter/filter.c -o filter.o
llvm-strip -g filter.o
```

**macOS fallback:** eBPF is Linux-only. On macOS, start a `squid` proxy sidecar configured with the allowlist. See Section 8, Constraint 2.

**Tests:**
- Domain resolution returns ≥ 1 IP for every default language profile domain
- Compiled bytes start with ELF magic (`\x7fELF`)
- Multi-language union allowlist compiles without error (node + rust domains combined)
- macOS: test generates squid config without attempting eBPF compile (mock `cfg!(target_os = "macos")`)

---

### Phase 4: Kernel Builder + Upgrade + Air-Gap Support

**Goal:** Build micro-Linux kernel with language toolchain. Support `upgrade-kernel` (segment surgery preserving all non-kernel segments). Support air-gap via kernel import and local cache.

**File:** `kernels/build.sh`

```bash
#!/usr/bin/env bash
# Builds a micro-Linux kernel for one or more language profiles.
# Multiple profiles install all toolchains into one image.
# Usage: ./build.sh --lang node@22 --lang rust@1.87 --output kernels/output/node-22_rust-1.87/bzImage

set -euo pipefail

LANGS=()
OUTPUT=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --lang)   LANGS+=("$2"); shift 2 ;;
        --output) OUTPUT="$2";   shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

mkdir -p "$(dirname "$OUTPUT")"
# Build multi-stage Dockerfile combining all language toolchain profile fragments
# from kernels/profiles/<lang>.dockerfile, producing a bootable bzImage at $OUTPUT
```

**File:** `crates/claudebox-rvf/src/kernel_builder.rs`

```rust
pub struct KernelBuilder {
    pub profiles: Vec<SingleProfile>,
    pub project_id: String,
    pub ssh_public_key: String,
}

impl KernelBuilder {
    pub async fn build(&self) -> anyhow::Result<PathBuf>;

    // Cache key: profiles sorted by lang+version, joined by "_"
    // e.g. "node-22_rust-1.87" — deterministic regardless of input order
    pub fn cache_key(&self) -> String;

    // Returns Some(path) if cache hit, None otherwise
    // Cache location: ~/.claudebox/kernels/<cache_key>/bzImage
    pub fn cached_path(&self) -> Option<PathBuf>;

    pub fn cache_dir(&self) -> PathBuf;
}
```

**REMEDIATION BS-2: `claudebox upgrade-kernel` — segment surgery**

Rebuilds KERNEL_SEG only. All other segments (META, VEC, INDEX, WITNESS, CRYPTO, EBPF) are preserved exactly. Uses `InitTransaction` for atomicity.

```rust
// crates/claudebox-rvf/src/kernel_upgrade.rs

pub struct KernelUpgrader {
    pub rvf_path: PathBuf,
    pub kernel_builder: KernelBuilder,
}

impl KernelUpgrader {
    pub async fn upgrade(&self) -> anyhow::Result<UpgradeResult> {
        // 1. Hash existing KERNEL_SEG content for audit record
        let from_hash = self.hash_current_kernel()?;

        // 2. Build fresh kernel (uses cache if available, else runs build.sh)
        let new_kernel_path = self.kernel_builder.build().await?;
        let to_hash = self.hash_file(&new_kernel_path)?;

        // 3. Extract all non-KERNEL segments from existing .rvf
        let segments = self.extract_non_kernel_segments()?;

        // 4. Write upgraded .rvf via InitTransaction for atomicity
        let mut tx = InitTransaction::new(&self.project_name()?, self.rvf_path.parent().unwrap())?;
        self.build_upgraded_rvf(tx.tmp_path(), &new_kernel_path, &segments)?;

        // 5. Append KernelUpgrade witness entry
        self.append_witness(tx.tmp_path(), WitnessEvent::KernelUpgrade {
            from_hash: from_hash.clone(),
            to_hash: to_hash.clone(),
        })?;

        // 6. Update kernel_built_at in MANIFEST_SEG
        self.update_manifest_timestamp(tx.tmp_path())?;

        // 7. Atomic commit
        tx.commit()?;

        Ok(UpgradeResult { from_hash, to_hash })
    }
}
```

**REMEDIATION BS-6: Air-gap support**

```rust
// crates/claudebox-rvf/src/kernel_import.rs

pub struct KernelImporter;

impl KernelImporter {
    // Extracts KERNEL_SEG from an existing .rvf and seeds the local cache.
    // No Docker or internet required — for air-gap environments.
    pub async fn import_from_rvf(source_rvf: &Path, cache_key: &str) -> anyhow::Result<PathBuf>;

    // Seeds cache from a raw bzImage file
    pub async fn import_from_file(kernel_path: &Path, cache_key: &str) -> anyhow::Result<PathBuf>;
}
```

CLI commands for kernel management:
```
claudebox kernel list                   # list all cached kernels with size and age
claudebox kernel pull node@22           # download pre-built kernel (requires internet)
claudebox kernel import <source.rvf>    # seed cache from existing appliance (air-gap)
claudebox upgrade-kernel <project.rvf>  # rebuild KERNEL_SEG, preserve all other segments
```

**Kernel staleness warning:** `claudebox status` reads `kernel_built_at` from MANIFEST_SEG. If older than 90 days (configurable in `~/.claudebox/config.toml`):
```
⚠ Kernel last built 94 days ago. Run `claudebox upgrade-kernel myproject.rvf` to update.
```

**Tests:**
- `KernelBuilder::cache_key` for `[rust@1.87, node@22]` equals `[node@22, rust@1.87]` (sorted)
- Cache hit skips `build.sh` invocation entirely
- `KernelUpgrader::upgrade` output has updated `kernel_built_at` and `KernelUpgrade` WITNESS entry
- `KernelUpgrader::upgrade` output has identical META_SEG and VEC_SEG to input
- `KernelImporter::import_from_rvf` seeds cache without invoking Docker
- Integration test (`#[ignore]`): full kernel build if Docker is available

---

### Phase 5: Firecracker Lifecycle + Session Lock + Real-Time Logs

**Goal:** Boot a `.rvf` in Firecracker, mount workspace via virtio-fs, establish SSH shell bridge, write session lock, stream logs via vsock.

**File:** `crates/claudebox-firecracker/src/vm.rs`

```rust
pub struct FirecrackerVm {
    pub project_id: String,
    pub rvf_path: PathBuf,
    pub workspace_path: PathBuf,
    pub manifest: ClaudeBoxManifest,
    socket_path: PathBuf,           // /tmp/claudebox-<project-id>.sock
}

impl FirecrackerVm {
    pub fn new(rvf_path: PathBuf, workspace_path: PathBuf) -> anyhow::Result<Self>;
    pub async fn extract_kernel(&self) -> anyhow::Result<PathBuf>;
    pub async fn configure(&self, kernel_path: &Path) -> anyhow::Result<()>;
    pub async fn start(&self) -> anyhow::Result<VmHandle>;
    pub async fn stop(&self, handle: &VmHandle) -> anyhow::Result<()>;
    pub async fn status(&self) -> anyhow::Result<VmStatus>;
}

pub struct VmHandle {
    pub pid: u32,
    pub ssh_port: u16,
    pub mcp_port: u16,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

pub enum VmStatus {
    Running { handle: VmHandle },
    Stopped,
    NotFound,
}
```

**Firecracker configuration sequence** (implement in `configure()`):
```rust
// PUT /boot-source    { kernel_image_path, boot_args: "console=ttyS0 reboot=k panic=1 pci=off" }
// PUT /drives/rootfs  { drive_id, path_on_host, is_root_device: true, is_read_only: false }
// PUT /machine-config { vcpu_count, mem_size_mib }
// PUT /network-interfaces/eth0  { iface_id, host_dev_name, guest_mac }
// PUT /vsock          { guest_cid: 3, uds_path: "/tmp/claudebox-<id>.vsock" }
// PUT /actions        { action_type: "InstanceStart" }
```

**virtio-fs workspace mount:** Start `virtiofsd` as a sidecar process before Firecracker boots:
```rust
Command::new("virtiofsd")
    .args(["--socket-path", virtiofs_socket, "--shared-dir", workspace_path.to_str().unwrap()])
    .spawn()?;
```

**REMEDIATION BS-4: Session lock file**

```rust
// crates/claudebox-firecracker/src/session_lock.rs

pub struct SessionLock {
    lock_path: PathBuf,   // /workspace/.claudebox/session.lock
}

#[derive(Serialize, Deserialize)]
pub struct LockFileContents {
    pub pid: u32,
    pub vm_id: String,
    pub started_at: String,     // ISO8601
    pub ssh_port: u16,
}

impl SessionLock {
    // Acquires lock. Errors with clear message if an active session is already running.
    // Clears stale lock (dead PID) automatically before acquiring.
    pub fn acquire(workspace: &Path, contents: &LockFileContents) -> anyhow::Result<Self>;

    // Removes lock file. Called on graceful shutdown.
    pub fn release(self) -> anyhow::Result<()>;

    // Returns Some(contents) if lock exists, None otherwise.
    pub fn check(workspace: &Path) -> anyhow::Result<Option<LockFileContents>>;
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Best-effort release on drop — handles panics and early returns
        let _ = std::fs::remove_file(&self.lock_path);
    }
}
```

Write a git pre-commit hook into the workspace on `claudebox init`:

```bash
# .git/hooks/pre-commit — written by claudebox init, mode 0o755
#!/usr/bin/env bash
LOCK=".claudebox/session.lock"
if [ -f "$LOCK" ]; then
    echo "⚠ ClaudeBox session active. Concurrent commits may conflict with Claude's changes."
    echo "  Stop the session with: claudebox stop <project.rvf>"
    echo "  To commit anyway:      git commit --no-verify"
    exit 1
fi
```

**REMEDIATION BS-11: Real-time observability — vsock log stream**

The Firecracker vsock device (CID 3, port 9999) carries a structured log stream from VM to host.

`claudebox-logd` (baked into KERNEL_SEG, < 500 KB compiled) hooks:
- Shell `PROMPT_COMMAND` / `precmd` for command capture
- `inotify` on `/workspace` for file write/delete events

It writes JSON-lines to vsock:
```json
{"ts":"2026-05-19T09:01:14Z","type":"CMD","data":{"cmd":"cargo build --release","pid":1234}}
{"ts":"2026-05-19T09:01:22Z","type":"FILE","data":{"path":"src/auth.rs","op":"write","bytes":1248}}
{"ts":"2026-05-19T09:01:31Z","type":"STDOUT","data":{"line":"test auth::test_jwt_valid ... ok"}}
```

`claudebox logs myproject.rvf --follow` connects to the vsock UDS socket on the host and pretty-prints:
```
[09:01:14] CMD    cargo build --release
[09:01:22] FILE   src/auth.rs written (1.2 KB)
[09:01:31] OUT    test auth::test_jwt_valid ... ok
```

**Tests:**
- `SessionLock::acquire` errors when lock file contains a live PID
- `SessionLock::acquire` succeeds and clears lock when file contains a dead PID
- `SessionLock::drop` removes lock file on panic (via `catch_unwind`)
- vsock log output is valid JSON-lines (unit test with mock vsock writer)
- Firecracker config JSON has correct shape for each PUT endpoint

---

### Phase 6: VEC_SEG Indexing Pipeline + Stale Entry Reconciliation

**Goal:** Index workspace files into VEC_SEG. Sweep stale entries for deleted files on boot. Compact on explicit `claudebox compact`.

**File:** `crates/claudebox-vec/src/indexer.rs`

```rust
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct WorkspaceIndexer {
    pub workspace_path: PathBuf,
    pub rvf_path: PathBuf,
    model: TextEmbedding,
}

impl WorkspaceIndexer {
    pub fn new(workspace_path: PathBuf, rvf_path: PathBuf) -> anyhow::Result<Self> {
        let model = TextEmbedding::try_new(InitOptions {
            model_name: EmbeddingModel::AllMiniLML6V2,
            show_download_progress: true,
            ..Default::default()
        })?;
        Ok(Self { workspace_path, rvf_path, model })
    }

    pub async fn index_all(&self) -> anyhow::Result<IndexStats>;
    pub async fn index_changed(&self, since: chrono::DateTime<chrono::Utc>) -> anyhow::Result<IndexStats>;
    pub async fn index_file(&self, path: &Path) -> anyhow::Result<Vec<ChunkRecord>>;
}

pub struct ChunkRecord {
    pub file_path: String,
    pub chunk_index: u32,
    pub text: String,
    pub embedding: Vec<f32>,    // 384-dim, all-MiniLM-L6-v2
    pub tombstoned: bool,       // REMEDIATION BS-5: soft deletion flag
}

pub struct IndexStats {
    pub files_indexed: usize,
    pub chunks_created: usize,
    pub files_tombstoned: usize,  // files tombstoned during this run
    pub elapsed_ms: u64,
}
```

**REMEDIATION BS-5: VEC_SEG stale entry reconciliation**

Run on every boot before the MCP server starts. Uses tombstoning for speed — no index rebuild on boot.

```rust
// crates/claudebox-vec/src/reconciler.rs

pub struct VecReconciler {
    pub workspace_path: PathBuf,
    pub rvf_path: PathBuf,
}

impl VecReconciler {
    // Fast metadata scan — does NOT recompute embeddings.
    // 1. List all unique file_path values in VEC_SEG chunk metadata
    // 2. For each path: check if file still exists on disk
    // 3. If missing: set tombstoned=true on all chunks for that file
    // 4. Append VecReconcile witness event with tombstoned file count
    // Physical removal happens only during `claudebox compact`.
    pub async fn reconcile(&self) -> anyhow::Result<ReconcileStats>;
}

pub struct ReconcileStats {
    pub files_checked: usize,
    pub files_tombstoned: usize,
}
```

The MCP `search_codebase` tool filters `tombstoned=true` at query time — deleted files never appear in results even before a compact.

`claudebox compact myproject.rvf` physically removes tombstoned entries and rebuilds the HNSW index. This is the only operation that modifies VEC_SEG structure destructively, and it uses `InitTransaction` for atomicity.

**Skip patterns** — never indexed:
```rust
const SKIP_PATTERNS: &[&str] = &[
    "node_modules/", ".git/", "target/", "__pycache__/",
    ".venv/", "dist/", "build/", ".next/", ".claudebox/",
    "*.lock", "*.bin", "*.jpg", "*.png", "*.gif",
    "*.wasm", "*.rvf", "*.zip", "*.tar.gz",
];
```

**Tests:**
- Reconciler tombstones chunks for deleted files, leaves existing file chunks intact
- `search_codebase` returns 0 results for a tombstoned file path
- Compact after reconciliation: tombstoned entries are physically absent, HNSW index is valid
- Skip patterns correctly exclude binary and ignored paths
- Chunk windowing: 512-token chunks with 64-token overlap produce correct boundaries

---

### Phase 7: MCP Server — VEC_SEG + META_SEG Query Interface

**Goal:** Expose codebase semantic search and session context as MCP tools for Claude Code. Run inside the VM. Detect and resolve port conflicts dynamically.

**REMEDIATION BS-9: MCP port conflict detection**

Before binding, check if the preferred port (7878) is already in use inside the VM:

```rust
// crates/claudebox-vec/src/mcp_server.rs

pub async fn find_mcp_port(preferred: u16) -> u16 {
    for port in preferred.. {
        if !port_check::is_local_ipv4_port_free(port) { continue; }
        return port;
    }
    unreachable!()
}
```

If the actual port differs from preferred, send a vsock message to the host process:
```json
{"type": "MCP_PORT_CHANGE", "actual_port": 7879, "preferred_port": 7878}
```

The `claudebox start` host process listens on vsock for this message (with a 2-second timeout, falling back to the preferred port) and calls `ShellBridge::write_mcp_config` with the actual port before launching `claude`.

**MCP Tools exposed:**

Tool 1: `search_codebase`
```json
{
  "name": "search_codebase",
  "description": "Semantic search over the project codebase. Returns the most relevant code chunks. Automatically excludes deleted files.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": { "type": "string" },
      "k":     { "type": "integer", "default": 5 }
    },
    "required": ["query"]
  }
}
```

Tool 2: `get_session_context`
```json
{
  "name": "get_session_context",
  "description": "Returns persisted session state from the last ClaudeBox session: task context, open files, scratchpad notes, and recent command history.",
  "inputSchema": { "type": "object", "properties": {} }
}
```

Base the MCP server on `@ruvector/rvf-mcp-server`. The `claudebox-mcp` binary inside the VM:
```bash
#!/usr/bin/env bash
# /usr/local/bin/claudebox-mcp — baked into kernel image
exec npx @ruvector/rvf-mcp-server \
  --transport stdio \
  --rvf /workspace/.claudebox/project.rvf \
  --port "${CLAUDEBOX_MCP_PORT:-7878}"
```

**Tests:**
- `find_mcp_port` returns 7879 when 7878 is artificially bound
- Port change vsock message is correctly formed JSON
- `search_codebase` returns 0 results for tombstoned file paths
- `get_session_context` returns correctly formatted `SessionState`

---

### Phase 8: CLI — `claudebox`

**Goal:** All user-facing subcommands wired up, including all commands added by remediations.

**File:** `crates/claudebox-cli/src/main.rs`

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "claudebox", version, about = "Isolated Claude Code project environments")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialise a new ClaudeBox project appliance
    Init {
        name: String,
        /// Language profiles, e.g. "node@22" or "node@22,rust@1.87"
        #[arg(long, default_value = "node@22", value_delimiter = ',')]
        lang: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        allow: Vec<String>,
        /// Import kernel from an existing appliance (air-gap / no Docker)
        #[arg(long)]
        kernel_from: Option<PathBuf>,   // REMEDIATION BS-6
    },

    /// Boot project VM and launch Claude Code interactively
    Start {
        rvf: PathBuf,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
    },

    /// Stop a running project VM gracefully
    Stop { rvf: PathBuf },

    /// Stream real-time logs from a running VM
    Logs {                              // REMEDIATION BS-11
        rvf: PathBuf,
        #[arg(long)]
        follow: bool,
        #[arg(long)]
        since: Option<String>,
    },

    /// Show VM and appliance status
    Status { rvf: PathBuf },

    /// Create a COW branch for safe experimentation
    Branch { rvf: PathBuf, branch_name: String },

    /// Rollback to last committed checkpoint
    Rollback { rvf: PathBuf },

    /// Display the cryptographic audit trail
    Audit {
        rvf: PathBuf,
        #[arg(long)]
        since: Option<String>,
        #[arg(long, default_value = "table")]
        format: String,
        /// Read a monthly archive file, e.g. --archive 2026-04
        #[arg(long)]
        archive: Option<String>,        // REMEDIATION BS-1
    },

    /// Manage named snapshots
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// Rebuild KERNEL_SEG preserving all other segments
    UpgradeKernel {                     // REMEDIATION BS-2
        rvf: PathBuf,
        #[arg(long)]
        lang: Option<String>,           // override version e.g. --lang node@23
    },

    /// Manage cached kernel images
    Kernel {                            // REMEDIATION BS-6
        #[command(subcommand)]
        action: KernelAction,
    },

    /// Migrate appliance to current RVF schema version
    Migrate { rvf: PathBuf },           // REMEDIATION BS-7

    /// Compact VEC_SEG and archive old WITNESS entries
    Compact { rvf: PathBuf },           // REMEDIATION BS-1 + BS-5

    /// Update network allowlist and recompile eBPF filter
    UpdateAllowlist {
        rvf: PathBuf,
        #[arg(long, value_delimiter = ',')]
        add: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        remove: Vec<String>,
    },

    /// Destroy a project appliance
    Destroy {
        rvf: PathBuf,
        #[arg(long)]
        archive_witness: bool,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    Create { rvf: PathBuf, name: String },
    List   { rvf: PathBuf },
    /// Export snapshot as a portable self-contained .rvf file
    Export {                            // REMEDIATION BS-12
        rvf: PathBuf,
        name: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore from an exported snapshot file
    Restore {
        snapshot_rvf: PathBuf,
        #[arg(long)]
        into: PathBuf,
    },
}

#[derive(Subcommand)]
enum KernelAction {
    List,
    Pull    { profile: String },
    Import  { source_rvf: PathBuf },
}
```

**`claudebox start` full sequence:**
1. Run preflight checks (`preflight::check_dependencies`)
2. Verify CRYPTO_SEG signature — error if tampered
3. Check schema version (`migrate::check_and_migrate`) — auto-migrate minor, prompt for major
4. Check for existing session lock — error if active session running, clear if stale
5. Run `WitnessCompactor::compact_if_needed` (automatic housekeeping on boot)
6. Extract kernel from KERNEL_SEG to temp path
7. Start `virtiofsd` sidecar for workspace mount
8. Configure and start Firecracker VM (with vsock configured)
9. Write session lock to workspace
10. Poll SSH port 2222 until ready (timeout 10 seconds)
11. Listen on vsock for `MCP_PORT_CHANGE` message (timeout 2 seconds, fall back to 7878)
12. Write `.claude/settings.json` and `.claude/mcp.json` via `ShellBridge`
13. Run `VecReconciler::reconcile` via SSH inside VM
14. On first boot only: run `WorkspaceIndexer::index_all` via SSH inside VM
15. Run `BootHook` via SSH — loads META_SEG, writes session context file
16. Write kernel staleness warning to stderr if `kernel_built_at` > 90 days
17. Launch `claude` interactively pointed at workspace directory
18. On Claude Code exit: run `ShutdownHook` via SSH — writes META_SEG
19. Release session lock
20. Stop Firecracker VM, kill virtiofsd, clean up TAP device and temp files

**`claudebox status` output:**
```
Project:     myproject
Appliance:   myproject.rvf (245 MB)
VM status:   Running (PID 18432, started 09:00:01)
SSH:         localhost:2222
MCP server:  localhost:7878

Language:    node@22, rust@1.87
Kernel age:  12 days  ✓
Witness:     1,247 entries (hot) + 2 archived months
VEC_SEG:     4,821 chunks across 312 files (3 tombstoned)
Schema:      v1 (current)

⚠ 3 tombstoned VEC entries. Run `claudebox compact myproject.rvf` to clean up.
```

**`claudebox audit` output:**
```
ClaudeBox Audit Trail: myproject.rvf
Chain integrity: ✓ verified (47 entries)

#   Timestamp              Event            Detail
1   2026-05-19 09:00:01   BOOT             project: myproject
2   2026-05-19 09:00:03   FILE_WRITE       src/main.rs (2.1 KB)
3   2026-05-19 09:00:15   COMMAND          cargo build (exit: 0)
4   2026-05-19 09:01:02   PKG_INSTALL      axum@0.7 from crates.io
...
```

---

### Phase 9: Session Persistence — Boot/Shutdown Hooks

**Goal:** META_SEG loaded at boot and written back at shutdown.

**Boot hook** (executed inside VM via SSH by `claudebox start`):
```bash
# /etc/claudebox/boot.sh — baked into kernel image
# 1. Read META_SEG from /workspace/.claudebox/project.rvf
# 2. Format as Claude context prefix
# 3. Write to /run/claudebox/session-context.txt (tmpfs)
# 4. Export CLAUDEBOX_MCP_PORT for MCP server process
```

**Shutdown hook** (executed inside VM via SSH by `claudebox stop`):
```bash
# /etc/claudebox/shutdown.sh
# 1. Collect command history from ~/.bash_history or ~/.zsh_history
# 2. mtime scan /workspace for files modified since boot timestamp
# 3. Serialise new SessionState
# 4. Write updated META_SEG to /workspace/.claudebox/project.rvf
```

```rust
// crates/claudebox-meta/src/hooks.rs

pub struct BootHook {
    pub rvf_path: PathBuf,
    pub session_context_output: PathBuf,    // /run/claudebox/session-context.txt
}

impl BootHook {
    pub fn run(&self) -> anyhow::Result<()>;
}

pub struct ShutdownHook {
    pub rvf_path: PathBuf,
    pub shell_history_path: PathBuf,
    pub workspace_path: PathBuf,
    pub boot_time: chrono::DateTime<chrono::Utc>,
}

impl ShutdownHook {
    pub fn run(&self) -> anyhow::Result<()>;
}
```

---

### Phase 10: Integration Tests

All tests are `#[ignore]` by default. Run with `cargo test -- --ignored` on a KVM host.

```rust
// tests/integration/test_init.rs
#[test]
#[ignore = "requires rvf-cli, clang, docker"]
fn test_init_single_language() { /* claudebox init myapp --lang node@22 */ }

#[test]
#[ignore = "requires rvf-cli, clang, docker"]
fn test_init_multi_language() {
    // claudebox init polyglot --lang node@22,rust@1.87
    // Verify Multi variant in MANIFEST_SEG
    // Verify union network policy has 5 unique domains
}

#[test]
#[ignore = "requires rvf-cli, clang"]
fn test_init_failure_leaves_no_artefacts() {
    // Simulate eBPF compile failure mid-init
    // Verify no .rvf or .rvf.tmp remains on disk
}

#[test]
#[ignore = "requires rvf-cli, clang, docker"]
fn test_init_kernel_from_existing_appliance() {
    // claudebox init newapp --lang node@22 --kernel-from existingapp.rvf
    // Verify init succeeds without calling Docker
    // Verify KERNEL_SEG in new appliance matches source
}

// tests/integration/test_witness.rs
#[test]
#[ignore = "requires rvf-cli"]
fn test_compaction_archives_old_entries() {
    // Create appliance, write 12,000 witness entries
    // Run WitnessCompactor (max_entries = 10,000)
    // Verify hot chain has <= 10,000 entries
    // Verify archive .rvf contains the excess
    // Verify both chains pass rvf verify-witness
}

#[test]
#[ignore = "requires rvf-cli"]
fn test_audit_archive_flag() {
    // claudebox audit myapp.rvf --archive 2026-04
    // Verify reads the monthly archive file
    // Verify chain integrity verified
}

// tests/integration/test_vec.rs
#[test]
#[ignore = "requires rvf-cli"]
fn test_reconciler_tombstones_deleted_files() {
    // Index workspace with 10 files
    // Delete 3 files from disk
    // Run VecReconciler
    // Verify 3 files tombstoned in VEC_SEG metadata
    // Verify search_codebase returns 0 results for deleted paths
}

#[test]
#[ignore = "requires rvf-cli"]
fn test_compact_removes_tombstoned_entries() {
    // Follow up from above: run claudebox compact
    // Verify tombstoned entries are physically absent
    // Verify HNSW index is valid and queryable
}

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
    // Verify boot succeeds
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_mcp_port_conflict_resolved() {
    // Start VM; artificially bind 7878 before MCP server starts
    // Verify MCP server binds on 7879
    // Verify .claude/mcp.json updated with 7879
}

// tests/integration/test_kernel.rs
#[test]
#[ignore = "requires rvf-cli, docker"]
fn test_upgrade_kernel_preserves_segments() {
    // claudebox init with session data in META_SEG
    // claudebox upgrade-kernel
    // Verify META_SEG identical to pre-upgrade
    // Verify WITNESS_SEG has KernelUpgrade event
    // Verify kernel_built_at updated
}

// tests/integration/test_migrate.rs
#[test]
#[ignore = "requires rvf-cli"]
fn test_migration_v1_to_v2() {
    // Create v1 schema appliance
    // Run claudebox migrate
    // Verify MANIFEST_SEG version == 2
    // Verify FormatMigrate event in WITNESS_SEG
    // Verify all original data intact
}

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

---

### Phase 11: Format Version Migration

**Goal:** Every `claudebox` operation checks schema version on open and migrates forward as needed.

```rust
// crates/claudebox-migrate/src/lib.rs

pub trait SegmentMigrator: Send + Sync {
    fn from_version(&self) -> u8;
    fn to_version(&self) -> u8;
    fn migrate(&self, store: &mut RvfStore) -> anyhow::Result<()>;
}

pub struct MigrationChain {
    migrators: Vec<Box<dyn SegmentMigrator>>,
}

impl MigrationChain {
    pub fn new() -> Self {
        Self {
            migrators: vec![
                Box::new(V1ToV2Migrator),
                // Add future migrators here as schema evolves
            ],
        }
    }

    // Applies all required migrations sequentially from current_version to latest.
    // Appends FormatMigrate witness event after each step.
    pub fn migrate_to_latest(&self, rvf_path: &Path, current_version: u8) -> anyhow::Result<u8>;

    pub fn needs_migration(&self, current_version: u8) -> bool;

    pub fn latest_version(&self) -> u8;
}

// Called by every claudebox command before operating on a .rvf file.
pub fn check_and_migrate(rvf_path: &Path, auto_migrate_minor: bool) -> anyhow::Result<()> {
    let manifest = read_manifest(rvf_path)?;
    let chain = MigrationChain::new();

    if !chain.needs_migration(manifest.version) {
        return Ok(());
    }

    let delta = chain.latest_version() - manifest.version;

    if delta == 1 || auto_migrate_minor {
        chain.migrate_to_latest(rvf_path, manifest.version)?;
        tracing::info!("Auto-migrated {} from schema v{} to v{}",
            rvf_path.display(), manifest.version, chain.latest_version());
    } else {
        anyhow::bail!(
            "Appliance uses schema v{}, but ClaudeBox requires v{}.\n\
             Run: claudebox migrate {}",
            manifest.version, chain.latest_version(), rvf_path.display()
        );
    }

    Ok(())
}
```

---

## 5. File Conventions

- All paths inside the VM use `/workspace` as project root
- ClaudeBox internal files: `/workspace/.claudebox/` (accessible but excluded from VEC_SEG indexing)
- Session lock: `/workspace/.claudebox/session.lock`
- Project RVF copy inside VM: `/workspace/.claudebox/project.rvf`
- Session context file: `/run/claudebox/session-context.txt` (tmpfs, never hits disk)
- Host `.rvf` is the source of truth — the VM is ephemeral
- All timestamps are UTC ISO8601
- Private keys: `~/.claudebox/keys/<project-id>.key` with mode `0o600`
- Temp init files: `<name>.rvf.tmp` — managed by `InitTransaction`, never visible to user
- `rvf-cli` invoked via `ClaudeboxRvfCli` wrapper for operations not in `rvf-runtime` Rust API

---

## 6. Error Handling Policy

- Use `anyhow::Result` for all fallible functions in binary crates
- Use `thiserror` for library crate error types
- All user-facing errors include: what failed, why it failed, what to do next
- Never `unwrap()` or `expect()` outside tests — use `?` and propagate
- Firecracker API errors include the full HTTP response body
- Boot failures clean up TAP device, virtiofsd process, and temp files before returning
- Init failures clean up automatically via `InitTransaction::drop` — no manual cleanup needed

---

## 7. Configuration File

`~/.claudebox/config.toml` — created on first `claudebox init`:

```toml
[defaults]
language = "node@22"
vcpus = 2
memory_mb = 4096
disk_gb = 20

[paths]
kernels   = "~/.claudebox/kernels"
keys      = "~/.claudebox/keys"
snapshots = "~/.claudebox/snapshots"
archive   = "~/.claudebox/archive"         # monthly witness archive files
sockets   = "/tmp/claudebox"

[firecracker]
binary = "firecracker"
jailer = false                              # set true for production hardening

[embedding]
model          = "all-MiniLM-L6-v2"
chunk_tokens   = 512
overlap_tokens = 64

[witness]
max_entries    = 10000
retention_days = 30

[kernel]
staleness_warn_days = 90
```

---

## 8. Known Constraints and Workarounds

**Constraint 1: macOS — no KVM**
Detect at startup. Use QEMU with HVF (Apple Hypervisor Framework) on macOS.

```rust
pub fn detect_hypervisor() -> Hypervisor {
    if Path::new("/dev/kvm").exists() {
        Hypervisor::Firecracker
    } else if cfg!(target_os = "macos") {
        Hypervisor::QemuHvf
    } else {
        panic!("No supported hypervisor. Install KVM (Linux) or ensure HVF is available (macOS).")
    }
}
```

**Constraint 2: macOS — no eBPF**
eBPF is Linux-only. On macOS, start a `squid` proxy sidecar configured with the allowlist before the QEMU VM boots. Functionally equivalent to the eBPF filter, lower performance. Torn down on `claudebox stop`.

**Constraint 3: RVF API coverage**
Some RVF operations (e.g. `rvf derive`, `rvf compact`) are only in `rvf-cli`, not `rvf-runtime`. Use `ClaudeboxRvfCli` wrapper:

```rust
pub struct ClaudeboxRvfCli { binary: PathBuf }

impl ClaudeboxRvfCli {
    pub async fn derive(&self, source: &Path, output: &Path) -> anyhow::Result<()>;
    pub async fn verify_witness(&self, rvf: &Path) -> anyhow::Result<WitnessVerifyResult>;
    pub async fn inspect(&self, rvf: &Path) -> anyhow::Result<RvfInspectResult>;
    pub async fn compact(&self, rvf: &Path) -> anyhow::Result<()>;
}
```

**Constraint 4: Claude Code `-p` / `--print` flag**
ClaudeBox does **not** use `claude -p` or `claude --print`. Claude Code is launched interactively (`claude`) with its bash tool redirected into the VM via the SSH shell bridge. The `-p` flag is for headless use cases and is not part of the ClaudeBox session model.

---

## 9. Remediation Reference

Quick reference mapping each blind spot to its implementation location.

| ID | Blind Spot | Remediation | Implemented In |
|----|------------|-------------|----------------|
| BS-1 | WITNESS_SEG unbounded growth | `WitnessCompactor` — 30-day rolling window, 10K entry cap, monthly archive `.rvf` files | Phase 2, Phase 8 |
| BS-2 | Kernel security updates | `KernelUpgrader` — segment surgery preserving META/VEC/WITNESS; staleness warning in `status` | Phase 4, Phase 8 |
| BS-3 | Multi-language projects | `LanguageProfile::Multi` + `NetworkPolicy::for_profiles` union | Phase 1 |
| BS-4 | virtio-fs race condition | `SessionLock` RAII + git pre-commit hook + documented contract | Phase 5 |
| BS-5 | VEC_SEG stale deletions | `VecReconciler` boot sweep + tombstone filter in MCP queries + `compact` for physical removal | Phase 6, Phase 7 |
| BS-6 | Bootstrap / air-gap | `--kernel-from` flag + `KernelImporter` + `claudebox kernel` subcommands | Phase 4, Phase 8 |
| BS-7 | RVF format migration | `MigrationChain` + `check_and_migrate` called on every command + `claudebox migrate` | Phase 11, Phase 8 |
| BS-8 | Anthropic API endpoints | Non-issue — host-side calls, eBPF filter is VM-internal only | Phase 3 (scope note) |
| BS-9 | MCP port conflict | `find_mcp_port` dynamic allocation + vsock notification + runtime `.claude/mcp.json` update | Phase 7 |
| BS-10 | Init atomicity | `InitTransaction` RAII + `.rvf.tmp` atomic rename + `Drop` cleanup | Phase 2 |
| BS-11 | Real-time observability | `claudebox-logd` vsock daemon + `claudebox logs --follow` host-side tail | Phase 5, Phase 8 |
| BS-12 | Snapshot storage / export | `claudebox snapshot export` producing portable self-contained `.rvf` | Phase 8 |

---

## 10. Definition of Done

The implementation is complete when:

- [ ] `cargo build --workspace --release` succeeds with zero errors
- [ ] `cargo test --workspace` passes (all non-ignored tests)
- [ ] `cargo clippy --workspace` produces zero warnings
- [ ] `claudebox init myapp --lang node@22` produces a valid `.rvf` with all required segments
- [ ] `claudebox init polyglot --lang node@22,rust@1.87` produces valid Multi language profile with union network policy
- [ ] `claudebox init newapp --kernel-from existing.rvf` succeeds without invoking Docker
- [ ] Init failure (any phase) leaves zero artefacts on disk
- [ ] `claudebox start myapp.rvf` boots VM, writes `.claude/settings.json`, writes session lock
- [ ] `claudebox logs myapp.rvf --follow` streams real-time events from running VM
- [ ] `claudebox status myapp.rvf` shows kernel age warning if > 90 days and tombstone warning if > 0
- [ ] Double-start on same project errors with "already running" message
- [ ] Stale session lock from crashed VM is cleared automatically on next start
- [ ] `claudebox audit myapp.rvf` displays audit trail with chain integrity verified
- [ ] `claudebox audit myapp.rvf --archive 2026-04` reads monthly archive file
- [ ] `claudebox compact myapp.rvf` removes tombstoned VEC entries and archives excess WITNESS entries
- [ ] `claudebox upgrade-kernel myapp.rvf` rebuilds KERNEL_SEG, META_SEG and VEC_SEG intact
- [ ] `claudebox migrate myapp.rvf` applies pending schema migrations with WITNESS event recorded
- [ ] `claudebox snapshot export myapp.rvf pre-refactor --output ./snap.rvf` produces self-contained bootable file
- [ ] Integration test `test_boot_and_session_restore` passes on KVM host
- [ ] Integration test `test_mcp_port_conflict_resolved` passes on KVM host
- [ ] README.md covers: prerequisites, quickstart (init → start → branch → audit), architecture diagram, air-gap setup, macOS setup, remediation summary
