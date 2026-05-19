<!-- TURBO:AUTO-START -->
# Project: claudebox

*Auto-generated at 2026-05-19 12:26 — do not edit between markers*

## Git State
- **Branch:** `main`
- **Remote:** `https://github.com/dgdev25/claudebox.git`
- **Commits:** 73
- **Uncommitted changes:** 6 file(s)

### Recent Commits
```
994ed0c feat(rvf,firecracker): switch to RvfStore, add QEMU launcher and initramfs builder
90529fe feat: implement binary .rvf format and wire claudebox init
e9fc04d feat: merge implement/claudebox-v1 — ClaudeBox full implementation
c095483 feat(witness,firecracker): implement Ed25519 signature verification and socket-level VM status
8492dd8 fix(rvf): handle EXDEV in InitTransaction::commit — fall back to copy+remove across filesystems
08f0ef1 fix(codesec): apply all Critical/High/Medium/Low fixes from code review + security audit
362d9fb refactor(config): extract config_path() helper to deduplicate path construction
639b4e5 fix(migrate): guard future-version reads, fix tautological tests, remove TOCTOU in config
```

## Tech Stack
- Rust

### Key Dependencies (Cargo.toml)
- `rvf-runtime`
- `rvf-crypto`
- `rvf-types`
- `tokio`
- `clap`
- `serde`
- `serde_json`
- `ed25519-dalek`
- `rand`
- `sha3`
- `fastembed`
- `anyhow`
- `thiserror`
- `tracing`
- `tracing-subscriber`
- *...and more*

## Structure (top-level)
```
crates/
docs/
ebpf/
kernels/
Cargo.toml
CLAUDE.md
```

<!-- TURBO:AUTO-END -->





