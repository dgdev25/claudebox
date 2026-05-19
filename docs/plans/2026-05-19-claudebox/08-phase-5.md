# ClaudeBox — Phase 5: Firecracker Lifecycle + Session Lock + Real-Time Logs

> **Prerequisite:** Phase 4 complete and verified.
> After completing this phase: SessionLock RAII tests pass, vsock log format tests pass.

---

### Task 5.1: SessionLock RAII

**Files:**
- Create: `crates/claudebox-firecracker/Cargo.toml`
- Create: `crates/claudebox-firecracker/src/session_lock.rs`
- Create: `crates/claudebox-firecracker/src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_acquire_errors_on_live_pid() {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    // Write a lock file with our own PID (definitely alive)
    let contents = LockFileContents {
        pid: std::process::id(),
        vm_id: "vm-test".into(),
        started_at: "2026-05-19T09:00:00Z".into(),
        ssh_port: 2222,
    };
    let json = serde_json::to_string(&contents).unwrap();
    let lock_dir = workspace.join(".claudebox");
    std::fs::create_dir_all(&lock_dir).unwrap();
    std::fs::write(lock_dir.join("session.lock"), json).unwrap();

    let result = SessionLock::acquire(workspace, &contents);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("already") || msg.contains("running"));
}

#[test]
fn test_acquire_clears_stale_lock_on_dead_pid() {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    // PID 999999 is almost certainly dead
    let stale = LockFileContents {
        pid: 999_999_999,
        vm_id: "vm-old".into(),
        started_at: "2026-05-01T00:00:00Z".into(),
        ssh_port: 2222,
    };
    let json = serde_json::to_string(&stale).unwrap();
    let lock_dir = workspace.join(".claudebox");
    std::fs::create_dir_all(&lock_dir).unwrap();
    std::fs::write(lock_dir.join("session.lock"), &json).unwrap();

    let new_contents = LockFileContents {
        pid: std::process::id(),
        vm_id: "vm-new".into(),
        started_at: "2026-05-19T09:00:00Z".into(),
        ssh_port: 2222,
    };
    let lock = SessionLock::acquire(workspace, &new_contents).unwrap();
    drop(lock);
}

#[test]
fn test_drop_removes_lock_file() {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    let contents = LockFileContents {
        pid: std::process::id(),
        vm_id: "vm-test".into(),
        started_at: "2026-05-19T09:00:00Z".into(),
        ssh_port: 2222,
    };
    let lock = SessionLock::acquire(workspace, &contents).unwrap();
    let lock_path = workspace.join(".claudebox/session.lock");
    assert!(lock_path.exists());
    drop(lock);
    assert!(!lock_path.exists());
}

#[test]
fn test_drop_on_panic_removes_lock() {
    let dir = tempdir().unwrap();
    let workspace = dir.path().to_owned();
    let contents = LockFileContents {
        pid: std::process::id(),
        vm_id: "vm-test".into(),
        started_at: "2026-05-19T09:00:00Z".into(),
        ssh_port: 2222,
    };
    let lock_path = workspace.join(".claudebox/session.lock");
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _lock = SessionLock::acquire(&workspace, &contents).unwrap();
        panic!("simulated panic");
    }));
    assert!(!lock_path.exists());
}
```

- [ ] **Step 2: Create `crates/claudebox-firecracker/Cargo.toml`**

```toml
[package]
name = "claudebox-firecracker"
version = "0.1.0"
edition = "2021"

[dependencies]
claudebox-core = { path = "../claudebox-core" }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
tokio = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Implement `session_lock.rs`** from Technical Plan §Phase 5

Key logic:
- `acquire()`: read existing lock if present → check if PID is alive (`kill(pid, 0)` → ESRCH means dead) → if alive: error; if dead or absent: write new lock
- `release()`: remove lock file
- `Drop`: best-effort `fs::remove_file`

- [ ] **Step 4: Run tests to verify pass**

```bash
cargo test -p claudebox-firecracker session_lock
# Expected: 4/4 pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-firecracker/
git commit -m "feat(firecracker): implement SessionLock RAII — prevents double-boot, clears stale locks"
```

---

### Task 5.2: FirecrackerVm — HTTP API configuration

**Files:**
- Create: `crates/claudebox-firecracker/src/vm.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_firecracker_config_json_shape() {
    // Verify the boot-source config serialises to the correct Firecracker API shape
    let boot_config = BootSourceConfig {
        kernel_image_path: "/tmp/vmlinux".into(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".into(),
    };
    let json = serde_json::to_value(&boot_config).unwrap();
    assert!(json["kernel_image_path"].as_str().is_some());
    assert!(json["boot_args"].as_str().unwrap().contains("console=ttyS0"));
}
```

- [ ] **Step 2: Implement `vm.rs`**

Full `FirecrackerVm` as in Technical Plan §Phase 5, including:
- `FirecrackerVm { project_id, rvf_path, workspace_path, manifest, socket_path }`
- `configure()` — sends 6 PUT requests to Firecracker HTTP API via Unix socket
- `start()` → `PUT /actions { action_type: "InstanceStart" }` → returns `VmHandle`
- `stop()` → `PUT /actions { action_type: "SendCtrlAltDel" }` + wait for process exit
- `status()` → checks socket existence → `VmStatus::Running` / `Stopped` / `NotFound`
- `VmHandle { pid, ssh_port, mcp_port, started_at }`

virtiofsd sidecar launched via `Command::new("virtiofsd")` before Firecracker boots.

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-firecracker vm
git add crates/claudebox-firecracker/src/vm.rs
git commit -m "feat(firecracker): implement FirecrackerVm with HTTP API configuration"
```

---

### Task 5.3: claudebox-logd vsock daemon

**Files:**
- Create: `crates/claudebox-logd/Cargo.toml`
- Create: `crates/claudebox-logd/src/main.rs`

- [ ] **Step 1: Write failing test — log message JSON format**

```rust
#[test]
fn test_cmd_log_entry_is_valid_json() {
    let entry = LogEntry {
        ts: "2026-05-19T09:01:14Z".into(),
        kind: LogEntryKind::Cmd,
        data: serde_json::json!({ "cmd": "cargo build --release", "pid": 1234 }),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "CMD");
    assert!(v["data"]["cmd"].as_str().is_some());
}

#[test]
fn test_file_log_entry_is_valid_json() {
    let entry = LogEntry {
        ts: "2026-05-19T09:01:22Z".into(),
        kind: LogEntryKind::File,
        data: serde_json::json!({ "path": "src/auth.rs", "op": "write", "bytes": 1248 }),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "FILE");
}
```

- [ ] **Step 2: Create `crates/claudebox-logd/Cargo.toml`**

```toml
[package]
name = "claudebox-logd"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "claudebox-logd"
path = "src/main.rs"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 3: Implement `main.rs`**

`LogEntry { ts, kind: LogEntryKind, data: serde_json::Value }` — serialises to `{"ts":"...","type":"CMD"|"FILE"|"STDOUT","data":{...}}`

Main loop:
1. Connect to vsock (CID 2 = host, port 9999) via `/dev/vsock`
2. Hook `PROMPT_COMMAND` by writing to `/etc/profile.d/claudebox-logd.sh`
3. Watch `/workspace` with inotify for write/delete events
4. Write JSON-lines to vsock connection

- [ ] **Step 4: Run unit tests (JSON format only — no vsock required)**

```bash
cargo test -p claudebox-logd
# Expected: 2/2 format tests pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-logd/
git commit -m "feat(logd): implement claudebox-logd vsock JSON-lines log forwarder"
```

---

### Task 5.4: Host-side vsock log reader (claudebox logs --follow)

**Files:**
- Create: `crates/claudebox-firecracker/src/log_reader.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_log_entry_pretty_print() {
    let entry = LogEntry {
        ts: "2026-05-19T09:01:14Z".into(),
        kind: "CMD".into(),
        data: serde_json::json!({ "cmd": "cargo build" }),
    };
    let line = entry.pretty_print();
    assert!(line.contains("09:01:14"));
    assert!(line.contains("CMD"));
    assert!(line.contains("cargo build"));
}
```

- [ ] **Step 2: Implement `log_reader.rs`**

```rust
pub struct VsockLogReader { pub uds_path: PathBuf }

impl VsockLogReader {
    // Opens vsock UDS socket, streams JSON-lines, pretty-prints each line
    pub async fn follow(&self, writer: &mut impl tokio::io::AsyncWrite) -> anyhow::Result<()>;
    // Reads until timestamp
    pub async fn read_since(&self, since: chrono::DateTime<chrono::Utc>) -> anyhow::Result<Vec<LogEntry>>;
}
```

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-firecracker log_reader
git add crates/claudebox-firecracker/src/log_reader.rs
git commit -m "feat(firecracker): add VsockLogReader for claudebox logs --follow"
```

---

### Task 5.5: git pre-commit hook writer

**Files:**
- Create: `crates/claudebox-core/src/git_hooks.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_pre_commit_hook_is_executable() {
    let dir = tempdir().unwrap();
    // Create fake .git/hooks dir
    std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
    write_pre_commit_hook(dir.path()).unwrap();
    let hook_path = dir.path().join(".git/hooks/pre-commit");
    assert!(hook_path.exists());
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(&hook_path).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o755);
}
```

- [ ] **Step 2: Implement `git_hooks.rs`**

```rust
pub fn write_pre_commit_hook(workspace: &Path) -> anyhow::Result<()>;
// Writes the hook script from Technical Plan §Phase 5 with mode 0o755
```

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-core git_hooks
git add crates/claudebox-core/src/git_hooks.rs crates/claudebox-core/src/lib.rs
git commit -m "feat(core): add git pre-commit hook writer — warns on concurrent session"
```

---

### Task 5.6: claudebox-core `claudebox start` orchestrator

**Files:**
- Create: `crates/claudebox-core/src/start.rs`

- [ ] **Step 1: Write failing test for start sequence validation**

```rust
#[test]
fn test_start_options_default_workspace_is_current_dir() {
    let opts = StartOptions {
        rvf: PathBuf::from("myapp.rvf"),
        workspace: PathBuf::from("."),
    };
    assert_eq!(opts.workspace, PathBuf::from("."));
}
```

- [ ] **Step 2: Implement `start.rs`**

```rust
pub struct StartOptions {
    pub rvf: PathBuf,
    pub workspace: PathBuf,
}

pub async fn run_start(opts: StartOptions) -> anyhow::Result<()>;
```

`run_start` implements the full 20-step start sequence from the pseudocode in `01-pseudocode.md`.

- [ ] **Step 3: Run test and commit**

```bash
cargo test -p claudebox-core start
git add crates/claudebox-core/src/start.rs crates/claudebox-core/src/lib.rs
git commit -m "feat(core): implement run_start — full 20-step boot sequence orchestrator"
```
