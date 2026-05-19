# Remaining ClaudeBox Stubs

**Status (2026-05-19):** 29 of 29 stubs are implemented.

The previously remaining Tier-5 items are now landed:

| # | Stub | File | Current behavior |
|---|------|------|------------------|
| 24 | `Commands::Logs` | `claudebox-cli/src/main.rs` | Wired through `VsockLogReader` (`--follow`, `--since`) using `setup::instance_vsock_path(project_id)` |
| 27 | `KernelBuilder::build()` | `claudebox-rvf/src/kernel_builder.rs` | **No-Docker default policy**: cache/prebuilt kernel only; explicit error when cache is missing |
| 28 | `ApplianceBuilder::embed_ebpf()` | `claudebox-rvf/src/builder.rs` | Compiles/embeds when Linux toolchain is available; warns and skips when unavailable |
| 29 | `claudebox-logd main()` | `claudebox-logd/src/main.rs` | Async JSON-lines forwarder baseline (stdin input to stdout canonical log stream) |

## Notes

- Docker is not required for default ClaudeBox flows.
- Kernel provisioning defaults to `--kernel-from` or setup-provided cached prebuilt kernel.
- Runtime path derivation is centralized with `setup::instance_vsock_path`.
