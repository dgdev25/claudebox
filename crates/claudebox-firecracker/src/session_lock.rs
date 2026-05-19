use std::path::{Path, PathBuf};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct LockFileContents {
    pub pid: u32,
    pub vm_id: String,
    pub started_at: String,
    pub ssh_port: u16,
}

#[derive(Debug)]
pub struct SessionLock {
    lock_path: PathBuf,
}

/// Check if a PID is alive using `kill -0` (POSIX; Linux + macOS only).
///
/// Returns false if the process does not exist or is not accessible.
/// ClaudeBox targets Linux and macOS only — Windows is out of scope for v1.
fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl SessionLock {
    /// Acquire the session lock for the given workspace.
    ///
    /// If a lock file exists and the PID is still alive, returns an error.
    /// If the lock file exists but the PID is dead (stale), removes the stale lock
    /// and proceeds.
    pub fn acquire(workspace: &Path, contents: &LockFileContents) -> anyhow::Result<Self> {
        let lock_dir = workspace.join(".claudebox");
        let lock_path = lock_dir.join("session.lock");

        if lock_path.exists() {
            let raw = std::fs::read_to_string(&lock_path)
                .map_err(|e| anyhow::anyhow!("failed to read lock file: {e}"))?;
            let existing: LockFileContents = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("failed to parse lock file: {e}"))?;

            if pid_is_alive(existing.pid) {
                anyhow::bail!(
                    "ClaudeBox session already running: PID {} (vm_id={}). \
                     Stop with: claudebox stop",
                    existing.pid,
                    existing.vm_id
                );
            }

            // Stale lock — remove it and proceed.
            std::fs::remove_file(&lock_path)
                .map_err(|e| anyhow::anyhow!("failed to remove stale lock: {e}"))?;
        }

        // Create the .claudebox directory with restricted permissions.
        std::fs::create_dir_all(&lock_dir)
            .map_err(|e| anyhow::anyhow!("failed to create .claudebox dir: {e}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&lock_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| anyhow::anyhow!("failed to set .claudebox permissions: {e}"))?;
        }

        let json = serde_json::to_string(contents)
            .map_err(|e| anyhow::anyhow!("failed to serialise lock contents: {e}"))?;

        // Use O_CREAT|O_EXCL (create_new) to atomically create the lock file,
        // eliminating the TOCTOU window between stale-lock removal and write
        // (CWE-367). Two concurrent `claudebox start` invocations cannot both
        // succeed: one will get EEXIST and fail.
        {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
                .map_err(|e| anyhow::anyhow!("failed to create lock file (concurrent start?): {e}"))?
                .write_all(json.as_bytes())
                .map_err(|e| anyhow::anyhow!("failed to write lock file: {e}"))?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| anyhow::anyhow!("failed to set lock file permissions: {e}"))?;
        }

        Ok(SessionLock { lock_path })
    }

    /// Explicitly release the lock (removes the lock file).
    pub fn release(self) -> anyhow::Result<()> {
        std::fs::remove_file(&self.lock_path)
            .map_err(|e| anyhow::anyhow!("failed to release session lock: {e}"))?;
        // Prevent Drop from attempting a second removal.
        std::mem::forget(self);
        Ok(())
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_acquire_errors_on_live_pid() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        // Write a lock file with our own PID (definitely alive).
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
        // PID 999999999 is almost certainly dead.
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
}
