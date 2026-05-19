<!-- TURBO:AUTO-START -->
# Project: claudebox

*Auto-generated at 2026-05-19 14:09 — do not edit between markers*

## Git State
- **Branch:** `main`
- **Remote:** `https://github.com/dgdev25/claudebox.git`
- **Commits:** 79
- **Uncommitted changes:** 10 file(s)

### Recent Commits
```
b5e5d93 feat(init): git safety net — commit .rvf on init, warn if no repo
ef63d00 feat(workspace): mount host project directory into VM via virtio-9p
cbe21b8 feat(distribution): add install.sh, release CI, and kernel download
ac97687 feat(images): add claudebox-dev qcow2 build pipeline
c86b073 feat(setup): add `claudebox setup` command for one-command installation
b5ab5fe feat(firecracker): wire QEMU launch, macOS initramfs fallback, and clean clippy
994ed0c feat(rvf,firecracker): switch to RvfStore, add QEMU launcher and initramfs builder
90529fe feat: implement binary .rvf format and wire claudebox init
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
images/
kernels/
scripts/
Cargo.toml
CLAUDE.md
```

<!-- TURBO:AUTO-END -->







