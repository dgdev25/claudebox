<!-- TURBO:AUTO-START -->
# Project: claudebox

*Auto-generated at 2026-05-19 15:50 — do not edit between markers*

## Git State
- **Branch:** `main`
- **Remote:** `https://github.com/dgdev25/claudebox.git`
- **Commits:** 87
- **Uncommitted changes:** 3 file(s)

### Recent Commits
```
586fe0f feat(stubs): merge Tier 3 stub implementations (stubs #13-#15)
a9bcded refactor(witness): dedupe JSONL loader, harden corrupt-key fallback, tighten tests
23b09c4 feat(stubs): implement Tier 3 stubs — genesis witness, allowlist update, witness compaction
964b222 feat(stubs): implement all Tier 1 + Tier 2 stubs (12 of 29 total)
8abfa99 refactor(core): extract parse_kernel_segment helper, tighten snapshot.rs
074e3c8 feat(stubs): implement Tier 2 stubs — snapshot, compact, branch/rollback, migrate
f93386c feat(stubs): implement Tier 1 stubs — stop, destroy, status, kernel, preflight, arch
418f057 feat(aarch64): native HVF boot on Apple Silicon with full virtio stack
```

### Other Active Branches
```
feat/tier3-stubs (2 minutes ago)
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








