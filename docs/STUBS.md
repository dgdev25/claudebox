# ClaudeBox — Stub Implementation Roadmap

28 stubs across 16 files, ordered easiest → hardest.
All items in Tier 1–2 require no rvf-runtime wiring.
Items in Tier 3+ require `RvfStore` segment I/O.
Items in Tier 5 require external toolchains or in-VM Linux context.

---

## Tier 1 — Trivial (no new dependencies, all infra already exists)

| # | Stub | File | What it needs |
|---|------|------|---------------|
| 1 | `Commands::Stop` | `claudebox-cli/src/main.rs:283` | Read `instance_pid_path()`, send SIGTERM, remove PID file |
| 2 | `Commands::Destroy` | `claudebox-cli/src/main.rs:331` | Reuse Stop logic, then `fs::remove_dir_all(~/.claudebox/vms/<id>/)` |
| 3 | `Commands::Status` | `claudebox-cli/src/main.rs:291` | Read PID file, `kill -0` via `session_lock.rs`, print arch + SSH port from manifest |
| 4 | `Commands::Kernel` (show/cache) | `claudebox-cli/src/main.rs:315` | Print `kernel.arch`, `kernel.ssh_port`, `kernel_built_at` from manifest JSON in `.rvf` |
| 5 | `preflight::check_single_dep` version check | `claudebox-core/src/preflight.rs:31` | Parse `--version` output, compare semver; `_min_version` param is unused |
| 6 | `KernelArch` hardcode fix | `claudebox-rvf/src/builder.rs:64` | Replace `KernelArch::X86_64 as u8` with match on `manifest.kernel.arch` |

---

## Tier 2 — Straightforward (no external services, qemu-img only)

| # | Stub | File | What it needs |
|---|------|------|---------------|
| 7 | `Commands::Snapshot` (create/list/restore/export) | `claudebox-cli/src/main.rs:307` | Wrap `qemu-img snapshot -c/-l/-a` + `qemu-img convert` for export against instance overlay |
| 8 | `Commands::Compact` (disk level) | `claudebox-cli/src/main.rs:323` | `qemu-img convert -c -O qcow2` overlay + atomic rename; witness/VEC compact is Tier 4 |
| 9 | `Commands::Branch` + `Commands::Rollback` | `claudebox-cli/src/main.rs:295/299` | Named qcow2 snapshots as branches; `qemu-img snapshot -a <name>` for rollback; depends on #7 |
| 10 | `check_and_migrate()` | `claudebox-migrate/src/lib.rs:94` | Open `.rvf` with `RvfStore::open_readonly`, extract manifest JSON, read `version` field, compare to chain |

---

## Tier 3 — Moderate (rvf-runtime segment I/O required)

| # | Stub | File | What it needs |
|---|------|------|---------------|
| 11 | `MigrationChain::migrate_to_latest()` | `claudebox-migrate/src/lib.rs:60` | Uncomment `m.migrate(store)?`; pass `RvfStore` handle in; chain logic is complete |
| 12 | `Commands::Migrate` | `claudebox-cli/src/main.rs:319` | Wire `check_and_migrate` into a user-visible report; depends on #10 + #11 |
| 13 | `ApplianceBuilder::write_genesis_witness()` | `claudebox-rvf/src/builder.rs:86` | Create `WitnessEntry` with `GenesisInit`, sign, call `store.embed_witness()`; `WitnessWriter` types complete |
| 14 | `run_update_allowlist()` | `claudebox-core/src/allowlist.rs:57` | Open `.rvf`, read manifest, update `allow_domains`, write back, append `AllowlistUpdate` witness event |
| 15 | `WitnessCompactor::compact_if_needed()` + `force_compact()` | `claudebox-witness/src/compaction.rs:72/84` | Read WITNESS_SEG, partition by retention window, archive old entries to monthly `.rvf` files; `partition_entries()` pure logic is already done |

---

## Tier 4 — Complex (multi-day, deep rvf-runtime + HNSW)

| # | Stub | File | What it needs |
|---|------|------|---------------|
| 16 | `Commands::Audit` | `claudebox-cli/src/main.rs:303` | Read + verify WITNESS_SEG hash chain, format entries; `--archive` reads monthly files; depends on #15 |
| 17 | `KernelUpgrader::extract_non_kernel_segments()` + `upgrade()` | `claudebox-rvf/src/kernel_upgrade.rs:46/66` | Full segment surgery: read all non-KERNEL segs, rebuild `.rvf` with new kernel via `InitTransaction`, append witness |
| 18 | `Commands::UpgradeKernel` | `claudebox-cli/src/main.rs:311` | CLI wrapper for #17 |
| 19 | `KernelImporter::import_from_rvf()` | `claudebox-rvf/src/kernel_import.rs:12` | Extract `KERNEL_SEG` bytes from source `.rvf`, write to local cache |
| 20 | `VecReconciler::reconcile()` | `claudebox-vec/src/reconciler.rs:27` | Walk workspace, diff against VEC_SEG chunk metadata, tombstone deleted files; pure `reconcile_chunks()` logic complete |
| 21 | `VecCompactor::compact()` | `claudebox-vec/src/compact.rs:21` | Read HNSW chunks from VEC_SEG, remove tombstoned, rebuild index, atomic write via `InitTransaction`; depends on #20 |

---

## Tier 5 — Hard (external toolchains or in-VM Linux context)

| # | Stub | File | What it needs |
|---|------|------|---------------|
| 22 | `WorkspaceIndexer::index_file()` / `index_changed()` / `index_all()` | `claudebox-vec/src/indexer.rs:121-133` | Chunk text, run `fastembed::TextEmbedding`, write vectors + metadata to VEC_SEG; fastembed in deps but not wired |
| 23 | `VsockLogReader::follow()` + `read_since()` | `claudebox-firecracker/src/log_reader.rs:30/41` | Connect to vsock UDS socket, stream JSON-lines; requires `claudebox-logd` running in VM |
| 24 | `Commands::Logs` | `claudebox-cli/src/main.rs:287` | CLI wrapper for #23; blocked on log_reader (#23) and logd (#28) |
| 25 | `BootHook::run()` + `ShutdownHook::run()` | `claudebox-meta/src/hooks.rs:10/25` | Read/write META_SEG containing session state; Claude Code hook integration |
| 26 | `handle_search_codebase()` + `handle_get_session_context()` | `claudebox-vec/src/mcp_server.rs:65/102` | Embed query with fastembed, run HNSW top-k search, filter tombstoned; depends on #22 |
| 27 | `KernelBuilder::build()` | `claudebox-rvf/src/kernel_builder.rs:16` | Invoke `kernels/build.sh` via Docker with language profiles to build custom bzImage |
| 28 | `ApplianceBuilder::embed_ebpf()` | `claudebox-rvf/src/builder.rs:81` | Compile eBPF C with `clang -target bpf`, embed via `store.embed_ebpf()`; `claudebox-ebpf::EbpfCompiler` exists on Linux |
| 29 | `claudebox-logd main()` | `claudebox-logd/src/main.rs:30` | Full in-VM daemon: vsock connect to host, inotify on `/workspace`, PROMPT_COMMAND hook, JSON-line forwarding; Linux-only, runs inside VM |

---

## Dependency Graph

```
#1 Stop
  └─ #2 Destroy
#3 Status
#4 Kernel
#5 preflight version check
#6 KernelArch fix

#7 Snapshot
  └─ #9 Branch/Rollback
#8 Compact (disk)
#10 check_and_migrate
  └─ #11 MigrationChain::migrate_to_latest
       └─ #12 Commands::Migrate

#13 write_genesis_witness
#14 run_update_allowlist
#15 WitnessCompactor
  └─ #16 Commands::Audit

#17 KernelUpgrader
  └─ #18 Commands::UpgradeKernel
#19 KernelImporter
#20 VecReconciler
  └─ #21 VecCompactor

#22 WorkspaceIndexer (fastembed)
  └─ #26 MCP search_codebase
#23 VsockLogReader
  └─ #24 Commands::Logs ──── also blocked on #29
#25 BootHook/ShutdownHook
#27 KernelBuilder (Docker)
#28 embed_ebpf (clang)
#29 claudebox-logd (in-VM)
  └─ #24 Commands::Logs
```

---

## Status Tracking

| # | Subject | Status |
|---|---------|--------|
| 1 | Commands::Stop | ✅ done |
| 2 | Commands::Destroy | ✅ done |
| 3 | Commands::Status | ✅ done |
| 4 | Commands::Kernel | ✅ done |
| 5 | preflight version check | ✅ done |
| 6 | KernelArch hardcode fix | ✅ done |
| 7 | Commands::Snapshot | ✅ done |
| 8 | Commands::Compact (disk) | ✅ done |
| 9 | Commands::Branch + Rollback | ✅ done |
| 10 | check_and_migrate | ✅ done |
| 11 | MigrationChain::migrate_to_latest | ✅ done |
| 12 | Commands::Migrate | ✅ done |
| 13 | write_genesis_witness | ✅ done |
| 14 | run_update_allowlist | ✅ done |
| 15 | WitnessCompactor | ✅ done |
| 16 | Commands::Audit | ✅ done |
| 17 | KernelUpgrader | ✅ done |
| 18 | Commands::UpgradeKernel | ✅ done |
| 19 | KernelImporter | ✅ done |
| 20 | VecReconciler | ✅ done |
| 21 | VecCompactor | ✅ done |
| 22 | WorkspaceIndexer (fastembed) | ✅ done (chunking + sidecar; embed feature gated) |
| 23 | VsockLogReader | ✅ done |
| 24 | Commands::Logs | ⬜ pending |
| 25 | BootHook / ShutdownHook | ✅ done |
| 26 | MCP search_codebase + session_context | ✅ done (substring search; cosine when embed feature on) |
| 27 | KernelBuilder (Docker) | ⬜ pending |
| 28 | embed_ebpf (clang) | ⬜ pending |
| 29 | claudebox-logd in-VM daemon | ⬜ pending |
