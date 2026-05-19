# Remaining ClaudeBox Stubs

**Status (2026-05-19):** 25 of 29 stubs landed on `main`. 4 stubs remain, all
Tier 5 — they require an external toolchain or in-VM Linux runtime that
the host dev loop cannot provide.

| # | Stub | File | Blocker |
|---|------|------|---------|
| 24 | `Commands::Logs` | `claudebox-cli/src/main.rs:294` | Blocked on #29 (no logs to read until logd writes them) |
| 27 | `KernelBuilder::build()` | `claudebox-rvf/src/kernel_builder.rs:16` | Docker + `kernels/build.sh` |
| 28 | `ApplianceBuilder::embed_ebpf()` | `claudebox-rvf/src/builder.rs:81` | `clang -target bpf`; Linux only via `claudebox-ebpf::EbpfCompiler` |
| 29 | `claudebox-logd main()` | `claudebox-logd/src/main.rs:30` | Linux-only in-VM daemon (vsock, inotify) |

---

## #24 `Commands::Logs`

**Where:** `claudebox-cli/src/main.rs` (the `Commands::Logs { rvf, follow, since }` arm).

**What to do once #29 is in place:**

1. Resolve the host-side UDS path the launcher used when starting the VM
   (currently the launcher in `claudebox-firecracker::qemu` writes the
   guest vsock to a host UDS — surface that path through a manifest
   field or `setup::instance_vsock_path(project_id)`).
2. `let reader = VsockLogReader { uds_path };` (already implemented in
   `claudebox-firecracker::log_reader`).
3. If `--follow`: `reader.follow(&mut tokio::io::stdout()).await?`.
4. Else if `--since <rfc3339>`: parse, call `reader.read_since(ts)`, print
   each entry's `pretty_print()` line.

**No new dependencies needed** — the reader is done. This is pure wiring.

---

## #27 `KernelBuilder::build()`

**Where:** `claudebox-rvf/src/kernel_builder.rs:16`.

**What it needs:**

- Spawn `docker run --rm -v <kernels-dir>:/out claudebox/kernel-build
  kernels/build.sh <lang> <version>` (the dev image already exists; see
  `images/`).
- The script produces `bzImage` + `initramfs` in the mounted volume.
- Move the artefacts into `~/.claudebox/kernels/<arch>/`.

**Why it's deferred:** requires a Docker daemon on the dev box. The
`kernels/build.sh` script and image are already committed; the host-side
shell-out has not been written.

**Test plan when wiring:** an integration test gated on
`#[cfg(feature = "docker-build")]` or `#[ignore = "requires docker"]`
that asserts the produced `bzImage` exists and is non-empty.

---

## #28 `ApplianceBuilder::embed_ebpf()`

**Where:** `claudebox-rvf/src/builder.rs:81`.

**What it needs:**

- Invoke `claudebox-ebpf::EbpfCompiler::compile(<source.c>, <target>)`,
  which calls `clang -target bpf -O2 -c <src> -o <obj>`.
- Pass the resulting bytecode (and optional BTF) into
  `RvfStore::embed_ebpf(program_type, attach_type, max_dim, &bytecode, btf_data)`.
- Surface the resulting `seg_id` to the caller via the builder's return.

**Why it's deferred:** clang must be installed on the host running
`claudebox init`; the `claudebox-ebpf` compiler only builds on Linux
today (no macOS path). For dev environments without clang, the function
should fall back to skipping eBPF embedding with a `tracing::warn`.

---

## #29 `claudebox-logd main()`

**Where:** `claudebox-logd/src/main.rs:30`.

**What it needs:** a full Linux daemon that runs *inside the guest VM*:

1. Bind a vsock listener on the well-known port the host bridge expects.
2. Watch `/workspace` with `inotify` (or `notify` crate with the inotify
   backend) for file create/write/delete events; emit `LogEntry`s.
3. Hook `PROMPT_COMMAND` in the guest shell to fork-exec a tiny helper
   that writes each command into a fifo the daemon tails.
4. Aggregate the streams; forward newline-delimited JSON over vsock to
   the host bridge.

**Why it's deferred:** the daemon is Linux-only (vsock + inotify), runs
inside the microVM rather than on the host, and currently has no CI
runner that boots a guest. The wire format matches what
`VsockLogReader` (#23) already parses, so once `logd` lands `Commands::Logs`
(#24) should just work.

---

## Notes for future implementers

- All sidecar paths are centralised in `claudebox-core::witness`
  (`witness_path_for_rvf`, `meta_sidecar_path`, `chunks_sidecar_path`,
  `manifest_sidecar_path`, `witness_archive_dir`).
- The `embed` feature on `claudebox-vec` is the gate for fastembed wiring;
  empty `ChunkRecord.embedding` is the contract for "no semantic search
  yet" and `mcp_server::handle_search_codebase` falls back to substring
  ranking when it sees that.
- Witness events for these stubs already exist
  (`WitnessEvent::VecReconcile`, `KernelUpgrade`, `WitnessCompact`); use
  `claudebox_core::witness::append_witness_entry` to record completion
  rather than building a new chain.
