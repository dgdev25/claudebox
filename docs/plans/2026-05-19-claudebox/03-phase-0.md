# ClaudeBox — Phase 0: SSH Shell Bridge

> **Prerequisite:** Git repo initialized, docs committed.
> This is the core integration mechanism — nothing else in ClaudeBox works without it.
> After completing this phase: `cargo test -p claudebox-core` must pass.

---

### Task 0.1: Workspace Cargo.toml

**Files:**
- Create: `Cargo.toml` (workspace root)

- [ ] **Step 1: Write failing test** — verify workspace compiles

```bash
cargo build --workspace 2>&1 | head -5
# Expected: errors about missing crates (workspace members don't exist yet)
```

- [ ] **Step 2: Create workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/claudebox-cli",
    "crates/claudebox-core",
    "crates/claudebox-rvf",
    "crates/claudebox-firecracker",
    "crates/claudebox-ebpf",
    "crates/claudebox-vec",
    "crates/claudebox-witness",
    "crates/claudebox-meta",
    "crates/claudebox-migrate",
    "crates/claudebox-logd",
]

[workspace.dependencies]
rvf-runtime = "0.2"
rvf-crypto = "0.2"
rvf-types = "0.1"
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand = "0.8"
sha3 = "0.10"
fastembed = "3"
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = { version = "0.4", features = ["serde"] }
notify = "6"
port-check = "0.2"
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "feat(workspace): add workspace Cargo.toml with all crate members"
```

---

### Task 0.2: claudebox-core crate skeleton

**Files:**
- Create: `crates/claudebox-core/Cargo.toml`
- Create: `crates/claudebox-core/src/lib.rs`

- [ ] **Step 1: Write failing test** — verify crate compiles

```bash
mkdir -p crates/claudebox-core/src
# Write minimal Cargo.toml and src/lib.rs, expect cargo test to pass with zero tests
```

- [ ] **Step 2: Create `crates/claudebox-core/Cargo.toml`**

```toml
[package]
name = "claudebox-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
ed25519-dalek = { workspace = true }
rand = { workspace = true }
sha3 = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
```

- [ ] **Step 3: Create `crates/claudebox-core/src/lib.rs`**

```rust
pub mod manifest;
pub mod preflight;
pub mod shell_bridge;
```

- [ ] **Step 4: Run test to verify it compiles**

```bash
cargo build -p claudebox-core
# Expected: succeeds (modules declared but not yet created will fail — that's OK at this step)
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-core/
git commit -m "feat(core): add claudebox-core crate skeleton"
```

---

### Task 0.3: ShellBridge — SSH script + settings.json writer

**Files:**
- Create: `crates/claudebox-core/src/shell_bridge.rs`

- [ ] **Step 1: Write failing tests first**

```rust
// crates/claudebox-core/src/shell_bridge.rs (test section)
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_install_shell_script_mode() {
        let dir = tempdir().unwrap();
        let bridge = ShellBridge {
            project_id: "test-id".into(),
            ssh_port: 2222,
            key_path: dir.path().join("test.key"),
            mcp_port: 7878,
        };
        bridge.install_shell_script().unwrap();
        let script_path = dirs::home_dir().unwrap()
            .join(".claudebox/bin/claudebox-shell");
        let meta = fs::metadata(&script_path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn test_write_claude_settings_valid_json() {
        let dir = tempdir().unwrap();
        let bridge = ShellBridge {
            project_id: "proj-abc".into(),
            ssh_port: 2222,
            key_path: PathBuf::from("/home/user/.claudebox/keys/proj-abc.key"),
            mcp_port: 7878,
        };
        bridge.write_claude_settings(dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(v["shell"].as_str().unwrap().ends_with("claudebox-shell"));
        assert_eq!(v["env"]["CLAUDEBOX_SSH_PORT"].as_str().unwrap(), "2222");
    }

    #[test]
    fn test_write_mcp_config_uses_actual_port() {
        let dir = tempdir().unwrap();
        let bridge = ShellBridge {
            project_id: "proj-abc".into(),
            ssh_port: 2222,
            key_path: PathBuf::from("/home/user/.claudebox/keys/proj-abc.key"),
            mcp_port: 7878,
        };
        bridge.write_mcp_config(dir.path(), 7879).unwrap();
        let content = fs::read_to_string(dir.path().join(".claude/mcp.json")).unwrap();
        assert!(content.contains("7879"));
        assert!(!content.contains("7878"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p claudebox-core shell_bridge -- --nocapture
# Expected: FAIL — ShellBridge not defined
```

- [ ] **Step 3: Implement `shell_bridge.rs`**

Full implementation from Technical Plan §Phase 0, including:
- `ShellBridge` struct with `project_id`, `ssh_port`, `key_path`, `mcp_port`
- `install_shell_script()` — writes `~/.claudebox/bin/claudebox-shell` with mode `0o755`
- `write_claude_settings(workspace)` — writes `.claude/settings.json`
- `write_mcp_config(workspace, actual_mcp_port)` — writes `.claude/mcp.json`

The shell script content:
```bash
#!/usr/bin/env bash
exec ssh \
  -i "${CLAUDEBOX_KEY_PATH}" \
  -p "${CLAUDEBOX_SSH_PORT}" \
  -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null \
  -o ConnectTimeout=5 \
  claude@127.0.0.1 "$@"
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p claudebox-core shell_bridge
# Expected: 3/3 tests pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-core/src/shell_bridge.rs
git commit -m "feat(core): implement ShellBridge — SSH script + claude settings writers"
```

---

### Task 0.4: preflight dependency checker

**Files:**
- Create: `crates/claudebox-core/src/preflight.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_missing_dep_returns_error() {
    // On CI / test machines without firecracker installed, this should return an error
    // not panic. Use a fake binary name to guarantee failure.
    let result = check_single_dep("definitely-not-a-real-binary-xyz123", "0.0.0");
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("definitely-not-a-real-binary-xyz123"));
    assert!(msg.contains("not found"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p claudebox-core preflight -- --nocapture
# Expected: FAIL — check_single_dep not defined
```

- [ ] **Step 3: Implement `preflight.rs`**

```rust
use anyhow::Result;

pub fn check_dependencies() -> Result<()> {
    check_single_dep("firecracker", "1.7.0")?;
    check_single_dep("virtiofsd", "0.1.0")?;
    check_single_dep("clang", "15.0.0")?;
    check_single_dep("rvf", "0.1.0")?;
    check_single_dep("ssh", "0.0.0")?;
    // On Linux only: check /dev/kvm
    #[cfg(target_os = "linux")]
    check_kvm()?;
    Ok(())
}

pub fn check_single_dep(name: &str, min_version: &str) -> Result<()> {
    use std::process::Command;
    let output = Command::new(name)
        .arg("--version")
        .output()
        .map_err(|_| anyhow::anyhow!(
            "Dependency '{}' not found. Install it with: <platform-specific instructions>", name
        ))?;
    // version check is best-effort; log if parse fails
    let _ = min_version; // version enforcement future work
    let _ = output;
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_kvm() -> Result<()> {
    if !std::path::Path::new("/dev/kvm").exists() {
        anyhow::bail!("/dev/kvm not found. Enable KVM: modprobe kvm_intel (or kvm_amd)");
    }
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p claudebox-core preflight
# Expected: PASS
```

- [ ] **Step 5: Verify full crate compiles clean**

```bash
cargo build -p claudebox-core
cargo clippy -p claudebox-core -- -D warnings
# Expected: zero errors, zero warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/claudebox-core/src/preflight.rs
git commit -m "feat(core): add preflight dependency checker"
```
