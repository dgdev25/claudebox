# ClaudeBox — Product Requirements Document

**Version:** 1.0
**Status:** Ready for Implementation
**Date:** 2026-05-19

---

## 1. Executive Summary

ClaudeBox is a per-project isolated execution environment for Claude Code workflows, packaged as a single portable `.rvf` (RuVector Format) file. It replaces the limitations of OpenAI Codex-style sandboxing (blanket network kill, no session persistence, no portable state) with a principled, layered isolation model that is simultaneously more secure and more developer-friendly.

Each Claude Code project runs inside a Firecracker microVM booted directly from a `.rvf` appliance file. That file contains the project's Linux kernel, toolchain, codebase embeddings (for Claude context), session state, network policy (enforced via eBPF), and a tamper-evident cryptographic audit trail — all in one portable binary.

**Core promise:** `claudebox start myproject.rvf` — one command, 125ms boot, full isolation, zero external dependencies.

---

## 2. Problem Statement

### 2.1 What Codex Does (and Why It Falls Short)

OpenAI Codex sandboxes Claude Code aggressively:
- Network killed entirely after dependency install phase
- No persistent session memory between runs
- No portable environment — tied to Codex's own infra
- No rollback mechanism short of git operations
- No audit trail of what the agent actually did
- Opinionated filesystem layout Claude Code can't easily influence

These constraints exist for legitimate security reasons but create a brittle developer experience. Real workflows need selective network access (package registries, local dev servers), session continuity, and the ability to experiment and roll back.

### 2.2 The Gap ClaudeBox Fills

| Requirement | Codex | Docker Alone | ClaudeBox |
|-------------|-------|--------------|-----------|
| Network isolation | Blanket kill | Manual proxy | eBPF allowlist in-kernel |
| Session persistence | None | Manual volume | META_SEG per-project |
| Portable environment | No | Partial (OCI image) | Single `.rvf` file |
| Rollback | Git only | Manual snapshots | COW branch files |
| Audit trail | None | None | Cryptographic WITNESS_SEG |
| Codebase context for Claude | None | None | VEC_SEG embeddings |
| Boot speed | N/A | 2–5s | ~125ms (Firecracker) |
| Air-gap capable | No | No | Yes |

---

## 3. Goals and Non-Goals

### 3.1 Goals

- **G1:** Each Claude Code project runs in a fully isolated Firecracker microVM booted from a `.rvf` appliance file
- **G2:** Network policy is enforced via eBPF (EBPF_SEG inside the `.rvf`) — not a userspace proxy
- **G3:** Session state persists between boots via META_SEG
- **G4:** Project codebase is indexed into VEC_SEG on first boot for Claude Code semantic context
- **G5:** COW branching (`rvf derive`) enables safe experimentation with instant rollback
- **G6:** Every Claude Code action is recorded in WITNESS_SEG — a tamper-evident cryptographic audit chain
- **G7:** A thin `claudebox` CLI manages the full lifecycle (init, start, branch, rollback, audit, destroy)
- **G8:** The entire environment ships as one file — no external registry, no compose files, no image tags

### 3.2 Non-Goals

- **NG1:** ClaudeBox is not a general-purpose container runtime — it is purpose-built for Claude Code workflows
- **NG2:** ClaudeBox does not replace Docker for production deployments
- **NG3:** Windows host support is not in scope for v1 (Linux and macOS via KVM/HVF)
- **NG4:** Multi-tenant shared environments are not in scope for v1
- **NG5:** ClaudeBox does not manage Claude API keys — the host environment handles auth

---

## 4. User Personas

### 4.1 Primary: Indie SaaS Developer
Running multiple concurrent Claude Code projects across different stacks. Needs per-project isolation so Claude Code in one project cannot affect another. Values session continuity — Claude should remember where it left off between sessions. Needs rollback before risky refactors. Typically runs on a Linux host with a discrete GPU.

### 4.2 Secondary: Agentic Pipeline Builder
Running Claude Code as an autonomous agent for automated issue resolution or code generation workflows. Needs a strong audit trail of what the agent did. Needs network allowlisting so the agent can reach package registries and version control hosts but not arbitrary endpoints. Needs CPU/memory limits to prevent runaway loops.

### 4.3 Tertiary: Team Lead / Reviewer
Receives a `.rvf` appliance from a team member. Boots it on their own machine and gets an identical, reproducible environment. Reviews the WITNESS_SEG audit chain to understand what Claude Code changed. Does not need to install project dependencies manually.

---

## 5. Functional Requirements

### 5.1 CLI — `claudebox`

#### `claudebox init`
```
claudebox init <project-name> [--lang node|python|rust|go] [--allow <domain,...>]
```
- Creates `<project-name>.rvf` in the current directory
- Writes MANIFEST_SEG with project identity, language profile, and network allowlist
- Embeds KERNEL_SEG: micro-Linux with language toolchain pre-installed
- Embeds EBPF_SEG: XDP socket filter enforcing the network allowlist
- Generates Ed25519 keypair, writes CRYPTO_SEG
- Initialises empty VEC_SEG (populated on first boot)
- Initialises empty META_SEG (session state, task history, scratchpad)
- Initialises WITNESS_SEG with genesis entry

#### `claudebox start`
```
claudebox start <project.rvf> [--port <n>] [--mount <host-path>:<guest-path>]
```
- Verifies CRYPTO_SEG signature before boot (tamper check)
- Boots the `.rvf` in a Firecracker microVM
- On first boot: indexes `/workspace` into VEC_SEG using `all-MiniLM-L6-v2` embeddings
- On subsequent boots: loads META_SEG session state, restores Claude context
- Mounts host project directory at `/workspace` via virtio-fs (bind-equivalent, live sync)
- Drops Claude Code into the session
- Records boot event to WITNESS_SEG

#### `claudebox branch`
```
claudebox branch <project.rvf> <branch-name>
```
- Calls `rvf derive` to create COW child: `<project>-<branch-name>.rvf`
- Only stores delta from parent — typically 2–10 MB overhead
- Records branch creation to parent WITNESS_SEG

#### `claudebox rollback`
```
claudebox rollback <project.rvf>
```
- Stops any running instance
- Reverts META_SEG and VEC_SEG to last committed checkpoint
- Records rollback event to WITNESS_SEG

#### `claudebox audit`
```
claudebox audit <project.rvf> [--since <timestamp>] [--format json|table]
```
- Reads and decodes WITNESS_SEG
- Displays: timestamp, event type, actor (claude/human), file paths affected, command run, network requests made
- Verifies hash chain integrity, flags any tampering

#### `claudebox snapshot`
```
claudebox snapshot <project.rvf> <snapshot-name>
```
- Commits current META_SEG and VEC_SEG state as a named checkpoint
- Stored as a derived `.rvf` in `~/.claudebox/snapshots/<project>/`

#### `claudebox destroy`
```
claudebox destroy <project.rvf>
```
- Stops running microVM
- Optionally archives WITNESS_SEG before deletion
- Removes `.rvf` file

### 5.2 Network Policy

- Default: block all egress except DNS
- Language profile defaults:
  - `node`: allow `registry.npmjs.org`, `nodejs.org`
  - `python`: allow `pypi.org`, `files.pythonhosted.org`
  - `rust`: allow `crates.io`, `static.crates.io`, `index.crates.io`
  - `go`: allow `proxy.golang.org`, `sum.golang.org`
- Custom domains added via `--allow` flag at init or appended post-init
- Allowlist compiled into eBPF bytecode and embedded in EBPF_SEG at init time
- eBPF filter runs at XDP layer — enforced before userspace, cannot be bypassed by agent

### 5.3 Session Persistence (META_SEG)

META_SEG stores the following per project:
```json
{
  "session": {
    "last_boot": "ISO8601",
    "working_dir": "/workspace/src",
    "open_files": ["src/main.rs", "Cargo.toml"],
    "task_context": "Implementing JWT middleware"
  },
  "history": [
    { "ts": "ISO8601", "type": "command", "value": "cargo build" },
    { "ts": "ISO8601", "type": "file_write", "path": "src/auth.rs" }
  ],
  "scratchpad": "Claude's free-form notes about the project",
  "installed_packages": ["axum@0.7", "tokio@1.38"]
}
```
On boot, this is injected into Claude Code's context as a system prompt prefix.

### 5.4 Codebase Embeddings (VEC_SEG)

- On first boot, ClaudeBox indexes all text files in `/workspace` using `all-MiniLM-L6-v2` (384-dim)
- Chunking: 512-token sliding window with 64-token overlap
- Index stored in VEC_SEG using RuVector HNSW
- On subsequent boots, only changed files (by mtime) are re-indexed (incremental)
- Claude Code can query VEC_SEG via the bundled `rvf-mcp-server` (stdio MCP transport) — this exposes semantic search as an MCP tool available to Claude Code

### 5.5 Resource Limits

Enforced at Firecracker VM level:
- vCPUs: 2 (configurable via MANIFEST_SEG)
- Memory: 4 GB (configurable)
- Disk: 20 GB sparse virtio-blk
- Network: 100 Mbps (via tc qdisc in EBPF_SEG)

### 5.6 Audit Trail (WITNESS_SEG)

Every WITNESS_SEG entry is a 64-byte chained record containing:
- Timestamp (nanosecond precision)
- Event type enum: `BOOT | COMMAND | FILE_WRITE | FILE_DELETE | NETWORK_REQ | SNAPSHOT | BRANCH | ROLLBACK | SHUTDOWN`
- SHA3-256 hash of event payload
- Hash of previous entry (chain linkage)
- Ed25519 signature over the entry

Chain integrity verified by `claudebox audit` and `rvf verify-witness`.

---

## 6. Non-Functional Requirements

| Requirement | Target |
|-------------|--------|
| Boot time | < 200ms from `claudebox start` to shell prompt |
| VEC_SEG index (10K files) | < 60s on first boot |
| VEC_SEG incremental update | < 5s per changed file |
| Network filter latency overhead | < 1µs (eBPF XDP) |
| `.rvf` file size (node project, empty) | < 500 MB |
| COW branch overhead | < 10 MB per branch |
| WITNESS_SEG write latency | < 100µs per entry |
| Host OS support | Linux (KVM), macOS (HVF via QEMU) |

---

## 7. Segment Map

```
<project>.rvf
├── MANIFEST_SEG   [4 KB]   Project identity, language profile, network allowlist, resource limits
├── KERNEL_SEG     [varies] Micro-Linux with language toolchain, SSH keys, boot config
├── EBPF_SEG       [~50 KB] Compiled eBPF/XDP network filter bytecode
├── VEC_SEG        [varies] Codebase embeddings (384-dim, all-MiniLM-L6-v2, HNSW index)
├── INDEX_SEG      [varies] HNSW progressive index for VEC_SEG
├── META_SEG       [~100 KB] Session state JSON
├── WITNESS_SEG    [grows]  Tamper-evident audit chain (64-byte chained records)
└── CRYPTO_SEG     [~2 KB]  Ed25519 public key + project signature
```

---

## 8. Security Model

### 8.1 Threat Model

| Threat | Mitigation |
|--------|-----------|
| Claude Code exfiltrates data via network | eBPF XDP allowlist — enforced below userspace |
| Agent modifies host filesystem outside workspace | Firecracker VM boundary — no host kernel access |
| Tampered `.rvf` shipped to colleague | CRYPTO_SEG signature verified before every boot |
| Runaway agent consumes all host resources | Firecracker vCPU/memory hard limits |
| Audit log deleted or altered | WITNESS_SEG hash chain — tampering is detectable |
| Agent installs malicious packages | Network allowlist limits package sources; WITNESS_SEG records installs |

### 8.2 Secret Handling

- API keys and tokens passed as environment variables at `claudebox start` time
- Written to `/run/secrets/` inside the VM (tmpfs — never touches disk)
- Not stored in META_SEG, VEC_SEG, or WITNESS_SEG
- Not persisted in the `.rvf` file

---

## 9. v1 Scope / MVP

Phase 1 (MVP):
- `claudebox init` with node, python, rust language profiles
- `claudebox start` with Firecracker boot and virtio-fs workspace mount
- `claudebox branch` + `claudebox audit`
- META_SEG session persistence
- Basic WITNESS_SEG (boot/shutdown/file_write events)
- eBPF network allowlist (pre-compiled, language-profile defaults)

Phase 2:
- VEC_SEG codebase indexing + rvf-mcp-server MCP tool for Claude
- Incremental VEC_SEG updates on file change
- `claudebox snapshot` named checkpoints
- Custom network allowlist at init time

Phase 3:
- RVM coherence domain integration (multi-project resource scheduling)
- SONA-based session learning (Claude adapts to project patterns over time)
- TEE enclave support (SGX/SEV-SNP for sensitive codebases)
- Web UI for audit trail visualisation

---

## 10. Success Metrics

- Boot time consistently < 200ms (P95)
- Zero instances of agent network egress outside allowlist in testing
- Session context successfully restored across 100% of reboots
- COW branch creation < 5 seconds for any project size
- WITNESS_SEG audit chain passes integrity verification in 100% of normal operations
- Developer can move a `.rvf` between two machines and boot successfully without any additional setup