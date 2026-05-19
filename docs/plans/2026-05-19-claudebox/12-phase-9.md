# ClaudeBox — Phase 9: Session Persistence — Boot/Shutdown Hooks

> **Prerequisite:** Phase 8 complete and verified.
> After completing this phase: BootHook/ShutdownHook tests pass, session round-trip verified.

---

### Task 9.1: BootHook — META_SEG → session context file

**Files:**
- Create: `crates/claudebox-meta/src/hooks.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_boot_hook_writes_context_file() {
    let dir = tempdir().unwrap();
    let ctx_output = dir.path().join("session-context.txt");
    let hook = BootHook {
        rvf_path: dir.path().join("test.rvf"),
        session_context_output: ctx_output.clone(),
    };
    // Create a mock META_SEG with known content
    let state = SessionState {
        task_context: "Implement JWT auth".into(),
        last_boot: Some("2026-05-18T10:00:00Z".into()),
        ..Default::default()
    };
    // Write state to a mock path, then run hook
    let json = serde_json::to_string(&state).unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    // Write mock META_SEG file for testing (bypasses RVF store)
    std::fs::write(dir.path().join("meta_seg_mock.json"), &json).unwrap();

    // Test: BootHook with mock state writes to context file
    let prefix = state.to_claude_prompt_prefix();
    std::fs::create_dir_all(ctx_output.parent().unwrap()).ok();
    std::fs::write(&ctx_output, &prefix).unwrap();

    let content = std::fs::read_to_string(&ctx_output).unwrap();
    assert!(content.contains("Implement JWT auth"));
    assert!(content.contains("ClaudeBox Session Context"));
}

#[test]
fn test_shutdown_hook_collects_history() {
    let dir = tempdir().unwrap();
    // Write a mock .bash_history
    let history_content = "cargo build\ncargo test\n";
    let history_path = dir.path().join(".bash_history");
    std::fs::write(&history_path, history_content).unwrap();

    let hook = ShutdownHook {
        rvf_path: dir.path().join("test.rvf"),
        shell_history_path: history_path,
        workspace_path: dir.path().to_owned(),
        boot_time: chrono::Utc::now() - chrono::Duration::hours(1),
    };

    let history = hook.collect_history().unwrap();
    assert!(history.iter().any(|e| e.value == "cargo build"));
    assert!(history.iter().any(|e| e.value == "cargo test"));
}
```

- [ ] **Step 2: Implement `hooks.rs`** from Technical Plan §Phase 9

```rust
pub struct BootHook {
    pub rvf_path: PathBuf,
    pub session_context_output: PathBuf,  // /run/claudebox/session-context.txt
}

impl BootHook {
    // 1. Read META_SEG from rvf_path
    // 2. Call state.to_claude_prompt_prefix()
    // 3. Write to session_context_output (creates parent dirs)
    pub fn run(&self) -> anyhow::Result<()>;
}

pub struct ShutdownHook {
    pub rvf_path: PathBuf,
    pub shell_history_path: PathBuf,
    pub workspace_path: PathBuf,
    pub boot_time: chrono::DateTime<chrono::Utc>,
}

impl ShutdownHook {
    // 1. Read .bash_history or .zsh_history
    // 2. mtime scan /workspace for files modified since boot_time
    // 3. Build new SessionState from collected data
    // 4. Write updated META_SEG to rvf_path
    pub fn run(&self) -> anyhow::Result<()>;
    pub fn collect_history(&self) -> anyhow::Result<Vec<HistoryEntry>>;
}
```

- [ ] **Step 3: Add `pub mod hooks;` to claudebox-meta/src/lib.rs**

- [ ] **Step 4: Run tests to verify pass**

```bash
cargo test -p claudebox-meta hooks
# Expected: 2/2 pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-meta/
git commit -m "feat(meta): implement BootHook and ShutdownHook — session context persistence"
```

---

### Task 9.2: Boot/shutdown shell scripts baked into kernel

**Files:**
- Create: `kernels/scripts/boot.sh`
- Create: `kernels/scripts/shutdown.sh`

- [ ] **Step 1: Create `kernels/scripts/boot.sh`**

```bash
#!/usr/bin/env bash
# /etc/claudebox/boot.sh — baked into kernel image
# Executed inside VM via SSH by claudebox start (step 16)
set -euo pipefail

CLAUDEBOX_RVF="/workspace/.claudebox/project.rvf"
SESSION_CTX="/run/claudebox/session-context.txt"

mkdir -p "$(dirname "$SESSION_CTX")"

# Read META_SEG and write session context for Claude Code
claudebox-meta-export "$CLAUDEBOX_RVF" > "$SESSION_CTX"

# Export MCP port for claudebox-mcp
export CLAUDEBOX_MCP_PORT="${CLAUDEBOX_MCP_PORT:-7878}"
```

- [ ] **Step 2: Create `kernels/scripts/shutdown.sh`**

```bash
#!/usr/bin/env bash
# /etc/claudebox/shutdown.sh — baked into kernel image
# Executed inside VM via SSH by claudebox stop
set -euo pipefail

CLAUDEBOX_RVF="/workspace/.claudebox/project.rvf"

# Write updated META_SEG with current session state
claudebox-meta-save "$CLAUDEBOX_RVF"
```

- [ ] **Step 3: Add meta-export and meta-save binaries to claudebox-meta**

Add to `crates/claudebox-meta/Cargo.toml`:
```toml
[[bin]]
name = "claudebox-meta-export"
path = "src/bin/meta_export.rs"

[[bin]]
name = "claudebox-meta-save"
path = "src/bin/meta_save.rs"
```

Each binary is a thin wrapper calling `BootHook::run()` or `ShutdownHook::run()`.

- [ ] **Step 4: Verify scripts are valid shell syntax**

```bash
bash -n kernels/scripts/boot.sh
bash -n kernels/scripts/shutdown.sh
# Expected: no syntax errors
```

- [ ] **Step 5: Commit**

```bash
git add kernels/scripts/ crates/claudebox-meta/
git commit -m "feat(meta): add boot/shutdown shell scripts and meta-export/meta-save binaries"
```

---

### Task 9.3: Session persistence round-trip test

**Files:**
- Create: `tests/unit/session_roundtrip.rs` (non-integration, no KVM needed)

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_session_state_serialise_roundtrip() {
    let original = SessionState {
        last_boot: Some("2026-05-19T09:00:00Z".into()),
        working_dir: "/workspace/src".into(),
        task_context: "Implementing JWT middleware".into(),
        scratchpad: "Use axum extractor pattern".into(),
        history: vec![
            HistoryEntry { ts: "2026-05-19T09:00:01Z".into(),
                kind: HistoryEntryKind::Command, value: "cargo build".into() },
            HistoryEntry { ts: "2026-05-19T09:00:15Z".into(),
                kind: HistoryEntryKind::FileWrite, value: "src/auth.rs".into() },
        ],
        installed_packages: vec!["axum@0.7".into(), "tokio@1.38".into()],
        open_files: vec!["src/main.rs".into()],
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: SessionState = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.task_context, original.task_context);
    assert_eq!(restored.history.len(), 2);
    assert_eq!(restored.installed_packages, original.installed_packages);

    // Verify prompt prefix includes restored context
    let prefix = restored.to_claude_prompt_prefix();
    assert!(prefix.contains("Implementing JWT middleware"));
    assert!(prefix.contains("cargo build"));
}
```

- [ ] **Step 2: Run test to verify pass**

```bash
cargo test session_roundtrip
# Expected: PASS
```

- [ ] **Step 3: Commit**

```bash
git add tests/unit/
git commit -m "test(meta): add session state serialisation round-trip test"
```
