# ClaudeBox — Plan Index

> **For agentic workers:** Load `subagent-driven` (recommended) or `executing-plans` from skills.yaml.
> Execute phases in order. Complete and verify each phase before starting the next.

**Goal:** Build ClaudeBox — a per-project isolated Firecracker microVM environment for Claude Code workflows, packaged as a single portable `.rvf` appliance file.

**Why:** Replaces blanket network kills and ephemeral sandboxing with principled isolation: eBPF allowlist, session persistence, COW branching, and a tamper-evident audit trail — all in one file, 125ms boot, zero external dependencies.

**Success criteria:**
- [ ] `cargo build --workspace --release` succeeds with zero errors
- [ ] `cargo test --workspace` passes (all non-ignored tests)
- [ ] `cargo clippy --workspace` produces zero warnings
- [ ] `claudebox init myapp --lang node@22` produces a valid `.rvf` with all 7 required segments
- [ ] `claudebox init polyglot --lang node@22,rust@1.87` produces Multi language profile with union network policy
- [ ] `claudebox init newapp --kernel-from existing.rvf` succeeds without invoking Docker
- [ ] Init failure (any phase) leaves zero artefacts on disk (InitTransaction)
- [ ] `claudebox start myapp.rvf` boots VM, writes `.claude/settings.json`, writes session lock
- [ ] `claudebox logs myapp.rvf --follow` streams real-time events from running VM
- [ ] `claudebox status myapp.rvf` shows kernel age warning and tombstone warning when applicable
- [ ] Double-start on same project errors with "already running" message
- [ ] Stale session lock from crashed VM cleared automatically on next start
- [ ] `claudebox audit myapp.rvf` displays audit trail with chain integrity verified
- [ ] `claudebox compact myapp.rvf` removes tombstoned VEC entries and archives excess WITNESS entries
- [ ] `claudebox upgrade-kernel myapp.rvf` rebuilds KERNEL_SEG with META/VEC intact
- [ ] `claudebox migrate myapp.rvf` applies schema migrations with WITNESS event recorded

**Out of scope:**
- Windows host support (v1: Linux + macOS only)
- Multi-tenant shared environments
- Claude API key management (host handles auth)
- Production deployment replacement for Docker
- TEE enclave support (Phase 3 roadmap only)
- Web UI for audit trail (Phase 3 roadmap only)

**Tech Stack:** Rust (cargo workspace), Firecracker microVM, RuVector/RVF format, eBPF/XDP (Linux) / squid (macOS), fastembed (all-MiniLM-L6-v2), Ed25519 + SHA3-256, tokio async runtime

## Document Map

| File | Phase | Tasks | Status |
|------|-------|-------|--------|
| 01-pseudocode.md | P — Pseudocode | — | [ ] |
| 02-architecture.md | A — Architecture | — | [ ] |
| 03-phase-0.md | R — SSH Shell Bridge | 4 | [ ] |
| 04-phase-1.md | R — Foundation: Core Types + Manifest | 5 | [ ] |
| 05-phase-2.md | R — RVF Builder + Init Atomicity + Witness Compaction | 7 | [ ] |
| 06-phase-3.md | R — eBPF Network Filter | 4 | [ ] |
| 07-phase-4.md | R — Kernel Builder + Upgrade + Air-Gap | 5 | [ ] |
| 08-phase-5.md | R — Firecracker Lifecycle + Session Lock + vsock Logs | 6 | [ ] |
| 09-phase-6.md | R — VEC_SEG Indexing + Stale Reconciliation | 5 | [ ] |
| 10-phase-7.md | R — MCP Server | 4 | [ ] |
| 11-phase-8.md | R — CLI Subcommands | 5 | [ ] |
| 12-phase-9.md | R — Session Persistence Hooks | 3 | [ ] |
| 13-phase-10.md | R — Integration Tests | 3 | [ ] |
| 14-phase-11.md | R — Format Version Migration + Completion | 4 | [ ] |

**Total tasks:** 60
**Implementation phases:** 12 (Phase 0 through Phase 11)

## Key Architectural Note

Claude Code runs on the **host machine** — not inside the VM. Its bash tool is redirected via SSH shell bridge (`claudebox-shell`) transparently into the Firecracker VM. Claude's Anthropic API calls are host-side and are never affected by the eBPF filter. The eBPF filter governs only what agent-executed code (npm install, cargo build, app HTTP) can reach from inside the VM.

## Spec References

- PRD: `docs/claudebox_prd.md`
- Technical Plan: `docs/claudebox-TECHNICAL-PLAN.md` (authoritative implementation reference — all data structures, build order, and tests are defined there)
