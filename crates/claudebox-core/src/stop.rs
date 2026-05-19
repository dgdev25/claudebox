use std::path::Path;
use anyhow::Result;

/// Read the QEMU PID from a PID file written by `claudebox start`.
pub fn read_pid_file(path: &Path) -> Result<u32> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("PID file not found at {}: {e}", path.display()))?;
    content
        .trim()
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("invalid PID in {}: {e}", path.display()))
}

/// Send SIGTERM to the QEMU process identified by `pid_path` and remove
/// the PID file on success. Returns an error if the process is not running
/// or the PID file is missing / corrupt.
pub fn stop_instance(pid_path: &Path) -> Result<()> {
    let pid = read_pid_file(pid_path)?;
    let status = std::process::Command::new("kill")
        .args(["-15", &pid.to_string()])
        .status()
        .map_err(|e| anyhow::anyhow!("failed to send SIGTERM to {pid}: {e}"))?;
    if !status.success() {
        anyhow::bail!("SIGTERM failed for PID {pid} — process may have already exited");
    }
    let _ = std::fs::remove_file(pid_path);
    Ok(())
}

/// Stop the VM (if running) then remove the entire per-instance VM directory
/// (`~/.claudebox/vms/<project_id>/`). Uses `force=true` to skip the stop step
/// when called without a running VM (e.g. after a crash).
pub fn destroy_instance(project_id: &str, force: bool) -> Result<()> {
    let pid_path = crate::setup::instance_pid_path(project_id);
    if !force && pid_path.exists() {
        stop_instance(&pid_path).ok();
    }
    let vm_dir = crate::setup::instance_vm_dir(project_id);
    if vm_dir.exists() {
        std::fs::remove_dir_all(&vm_dir)
            .map_err(|e| anyhow::anyhow!("failed to remove VM dir {}: {e}", vm_dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_read_pid_file_valid() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("qemu.pid");
        std::fs::write(&path, "12345").unwrap();
        assert_eq!(read_pid_file(&path).unwrap(), 12345u32);
    }

    #[test]
    fn test_read_pid_file_trims_whitespace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("qemu.pid");
        std::fs::write(&path, "  99999\n").unwrap();
        assert_eq!(read_pid_file(&path).unwrap(), 99999u32);
    }

    #[test]
    fn test_read_pid_file_missing_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("qemu.pid");
        let err = read_pid_file(&path).unwrap_err();
        assert!(
            err.to_string().contains("not found") || err.to_string().contains("No such file"),
            "expected 'not found' in error, got: {err}"
        );
    }

    #[test]
    fn test_read_pid_file_invalid_content_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("qemu.pid");
        std::fs::write(&path, "not-a-pid").unwrap();
        assert!(read_pid_file(&path).is_err());
    }

    #[test]
    fn test_destroy_instance_removes_vm_dir() {
        // Create the directory under the real data dir using a unique test ID.
        let project_id = format!("test-destroy-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos());
        let vm_dir = crate::setup::instance_vm_dir(&project_id);
        std::fs::create_dir_all(&vm_dir).unwrap();
        std::fs::write(vm_dir.join("disk.qcow2"), b"fake").unwrap();

        destroy_instance(&project_id, true).unwrap();
        assert!(!vm_dir.exists(), "VM dir should be removed after destroy");
    }

    #[test]
    fn test_destroy_instance_nonexistent_dir_is_ok() {
        // A project that was never created should succeed silently.
        destroy_instance("no-such-project-xyz-test-only", true).unwrap();
    }
}
