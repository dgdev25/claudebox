# ClaudeBox — Architecture

## Cargo Workspace Layout

```
claudebox/
├── Cargo.toml                         # workspace root
├── crates/
│   ├── claudebox-cli/                 # binary: `claudebox` command
│   │   └── src/main.rs
│   ├── claudebox-core/                # init, start, preflight, shell_bridge
│   │   └── src/{lib,manifest,preflight,shell_bridge}.rs
│   ├── claudebox-rvf/                 # appliance builder, kernel builder, transaction
│   │   └── src/{builder,transaction,kernel_builder,kernel_upgrade,kernel_import}.rs
│   ├── claudebox-firecracker/         # VM lifecycle, session lock
│   │   └── src/{vm,session_lock}.rs
│   ├── claudebox-ebpf/                # eBPF compile, domain resolution
│   │   └── src/{compiler,squid}.rs
│   ├── claudebox-vec/                 # embedding indexer, reconciler, MCP server
│   │   └── src/{indexer,reconciler,mcp_server}.rs
│   ├── claudebox-witness/             # witness writer, verifier, compactor
│   │   └── src/{lib,compaction}.rs
│   ├── claudebox-meta/                # session state serde, boot/shutdown hooks
│   │   └── src/{lib,hooks}.rs
│   ├── claudebox-migrate/             # format migration chain
│   │   └── src/lib.rs
│   └── claudebox-logd/                # vsock log forwarder (baked into kernel)
│       └── src/main.rs
├── ebpf/
│   └── network_filter/filter.c        # eBPF C source (XDP program)
├── kernels/
│   ├── build.sh                       # multi-language kernel builder (Docker)
│   └── profiles/                      # per-language Dockerfile fragments
│       ├── node.dockerfile
│       ├── python.dockerfile
│       ├── rust.dockerfile
│       └── go.dockerfile
├── tests/
│   └── integration/                   # #[ignore] end-to-end tests
└── docs/
    ├── claudebox_prd.md
    ├── claudebox-TECHNICAL-PLAN.md
    └── plans/2026-05-19-claudebox/    # this directory
```

## Component Map

| Crate | Responsibility |
|-------|----------------|
| `claudebox-cli` | Clap CLI parsing, subcommand dispatch |
| `claudebox-core` | `ClaudeBoxManifest`, `preflight::check_dependencies`, `ShellBridge` |
| `claudebox-rvf` | `ApplianceBuilder`, `InitTransaction` (RAII atomicity), `KernelBuilder`, `KernelUpgrader`, `KernelImporter`, `ClaudeboxRvfCli` (rvf-cli wrapper) |
| `claudebox-firecracker` | `FirecrackerVm` (HTTP API config + start/stop), `SessionLock` (RAII), `VmHandle`, `VmStatus` |
| `claudebox-ebpf` | `EbpfCompiler` (clang + llvm-strip), domain resolution to IPv4, squid config gen (macOS) |
| `claudebox-vec` | `WorkspaceIndexer` (fastembed, HNSW), `VecReconciler` (tombstoning), MCP server (`search_codebase`, `get_session_context`) |
| `claudebox-witness` | `WitnessEntry` (64-byte chained record), `WitnessCompactor` (rolling window + archive) |
| `claudebox-meta` | `SessionState` serde, `BootHook`, `ShutdownHook`, `SessionState::to_claude_prompt_prefix()` |
| `claudebox-migrate` | `SegmentMigrator` trait, `MigrationChain`, `check_and_migrate()` |
| `claudebox-logd` | vsock log daemon (runs inside VM): captures `PROMPT_COMMAND`, inotify on `/workspace` |

## Key Interfaces

```rust
// claudebox-core/src/manifest.rs
pub struct ClaudeBoxManifest { version, project_id, project_name, language, created_at,
    kernel_built_at, network, resources, kernel, witness }
pub enum LanguageProfile { Single(SingleProfile), Multi(Vec<SingleProfile>) }
pub struct NetworkPolicy { allow_domains: Vec<String>, allow_localhost: bool, dns_server: String }
impl NetworkPolicy { pub fn for_profiles(profiles: &[&SingleProfile]) -> Self }

// claudebox-rvf/src/transaction.rs
pub struct InitTransaction { tmp_path, final_path, cleanup_paths, committed }
impl InitTransaction { pub fn new(name, dir) -> Result<Self>; pub fn commit(self) -> Result<()> }
impl Drop for InitTransaction { /* removes .rvf.tmp on failure/panic */ }

// claudebox-firecracker/src/session_lock.rs
pub struct SessionLock { lock_path: PathBuf }
impl SessionLock { pub fn acquire(workspace, contents) -> Result<Self>;
    pub fn release(self) -> Result<()>; pub fn check(workspace) -> Result<Option<LockFileContents>> }
impl Drop for SessionLock { /* best-effort remove on drop */ }

// claudebox-witness/src/lib.rs
pub struct WitnessEntry { seq, ts_nanos, event, payload_hash, prev_hash, signature }
pub enum WitnessEvent { Boot, Shutdown, Command, FileWrite, FileDelete, NetworkRequest,
    PackageInstall, Snapshot, Branch, Rollback, KernelUpgrade, WitnessCompact,
    VecReconcile, FormatMigrate }

// claudebox-vec/src/indexer.rs
pub struct WorkspaceIndexer { workspace_path, rvf_path, model: TextEmbedding }
pub struct ChunkRecord { file_path, chunk_index, text, embedding: Vec<f32>, tombstoned: bool }

// claudebox-migrate/src/lib.rs
pub trait SegmentMigrator { fn from_version(&self) -> u8; fn to_version(&self) -> u8;
    fn migrate(&self, store: &mut RvfStore) -> Result<()> }
pub fn check_and_migrate(rvf_path: &Path, auto_migrate_minor: bool) -> Result<()>
```

## Dependency Graph

```
claudebox-cli
  → claudebox-core (manifest, preflight, shell_bridge)
  → claudebox-rvf (builder, transaction, kernel_*)
  → claudebox-firecracker (vm, session_lock)
  → claudebox-ebpf (compiler)
  → claudebox-vec (indexer, reconciler, mcp_server)
  → claudebox-witness (lib, compaction)
  → claudebox-meta (lib, hooks)
  → claudebox-migrate (lib)

claudebox-core → (no internal deps)
claudebox-rvf → claudebox-core, claudebox-witness
claudebox-firecracker → claudebox-core
claudebox-ebpf → (no internal deps)
claudebox-vec → claudebox-witness, claudebox-meta
claudebox-witness → claudebox-core
claudebox-meta → claudebox-witness
claudebox-migrate → claudebox-core, claudebox-witness
claudebox-logd → (standalone binary, no workspace deps)
```

## Files Changed (complete list)

| File | Action |
|------|--------|
| `Cargo.toml` | Create — workspace root |
| `crates/claudebox-cli/Cargo.toml` | Create |
| `crates/claudebox-cli/src/main.rs` | Create — all subcommands |
| `crates/claudebox-core/Cargo.toml` | Create |
| `crates/claudebox-core/src/lib.rs` | Create |
| `crates/claudebox-core/src/manifest.rs` | Create — all manifest types |
| `crates/claudebox-core/src/preflight.rs` | Create — dep checks |
| `crates/claudebox-core/src/shell_bridge.rs` | Create — SSH bridge |
| `crates/claudebox-rvf/Cargo.toml` | Create |
| `crates/claudebox-rvf/src/builder.rs` | Create — ApplianceBuilder |
| `crates/claudebox-rvf/src/transaction.rs` | Create — InitTransaction RAII |
| `crates/claudebox-rvf/src/kernel_builder.rs` | Create — KernelBuilder + cache |
| `crates/claudebox-rvf/src/kernel_upgrade.rs` | Create — KernelUpgrader |
| `crates/claudebox-rvf/src/kernel_import.rs` | Create — KernelImporter |
| `crates/claudebox-rvf/src/rvf_cli.rs` | Create — ClaudeboxRvfCli wrapper |
| `crates/claudebox-firecracker/Cargo.toml` | Create |
| `crates/claudebox-firecracker/src/vm.rs` | Create — FirecrackerVm |
| `crates/claudebox-firecracker/src/session_lock.rs` | Create — SessionLock RAII |
| `crates/claudebox-ebpf/Cargo.toml` | Create |
| `crates/claudebox-ebpf/src/compiler.rs` | Create — EbpfCompiler |
| `crates/claudebox-ebpf/src/squid.rs` | Create — macOS fallback |
| `crates/claudebox-vec/Cargo.toml` | Create |
| `crates/claudebox-vec/src/indexer.rs` | Create — WorkspaceIndexer |
| `crates/claudebox-vec/src/reconciler.rs` | Create — VecReconciler |
| `crates/claudebox-vec/src/mcp_server.rs` | Create — MCP tools |
| `crates/claudebox-witness/Cargo.toml` | Create |
| `crates/claudebox-witness/src/lib.rs` | Create — WitnessEntry + chain |
| `crates/claudebox-witness/src/compaction.rs` | Create — WitnessCompactor |
| `crates/claudebox-meta/Cargo.toml` | Create |
| `crates/claudebox-meta/src/lib.rs` | Create — SessionState |
| `crates/claudebox-meta/src/hooks.rs` | Create — BootHook + ShutdownHook |
| `crates/claudebox-migrate/Cargo.toml` | Create |
| `crates/claudebox-migrate/src/lib.rs` | Create — MigrationChain |
| `crates/claudebox-logd/Cargo.toml` | Create |
| `crates/claudebox-logd/src/main.rs` | Create — vsock log daemon |
| `ebpf/network_filter/filter.c` | Create — XDP program |
| `kernels/build.sh` | Create — Docker kernel build |
| `kernels/profiles/node.dockerfile` | Create |
| `kernels/profiles/python.dockerfile` | Create |
| `kernels/profiles/rust.dockerfile` | Create |
| `kernels/profiles/go.dockerfile` | Create |
| `tests/integration/*.rs` | Create — #[ignore] integration tests |

## Configuration File

`~/.claudebox/config.toml` — created on first `claudebox init`, controls defaults for
vcpus, memory, disk, kernel staleness threshold, embedding model params, witness retention.
See Technical Plan §7 for full TOML structure.

## `.rvf` Segment Map

```
<project>.rvf
├── MANIFEST_SEG   [4 KB]    ClaudeBoxManifest JSON
├── KERNEL_SEG     [varies]  bzImage + SSH keys + boot config
├── EBPF_SEG       [~50 KB]  compiled XDP bytecode (or squid config on macOS)
├── VEC_SEG        [varies]  HNSW index (384-dim f32 vectors)
├── INDEX_SEG      [varies]  HNSW progressive index metadata
├── META_SEG       [~100 KB] SessionState JSON
├── WITNESS_SEG    [grows]   64-byte chained Ed25519 records
└── CRYPTO_SEG     [~2 KB]   Ed25519 pubkey + project signature
```
