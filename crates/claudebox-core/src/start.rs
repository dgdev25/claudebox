use std::path::PathBuf;

pub struct StartOptions {
    pub rvf: PathBuf,
    pub workspace: PathBuf,
}

/// Execute the full 20-step `claudebox start` boot sequence.
///
/// Current status: Steps 1 (preflight) wired; remaining 19 steps are stubs
/// pending Phase 8 integration when all sub-systems are available.
///
/// Full sequence (from pseudocode §claudebox start):
///  1.  preflight::check_dependencies()
///  2.  Verify CRYPTO_SEG signature → error if tampered
///  3.  check_and_migrate(rvf_path) → auto-migrate minor versions
///  4.  SessionLock::check() → error if live PID, clear if stale PID
///  5.  WitnessCompactor::compact_if_needed() → archive if >10K or >30 days
///  6.  Extract KERNEL_SEG to /tmp/claudebox-<project_id>/bzImage
///  7.  Start virtiofsd sidecar (socket + workspace dir)
///  8.  Configure Firecracker VM via HTTP API (6 PUT requests)
///  9.  SessionLock::acquire(workspace, { pid, vm_id, started_at, ssh_port })
/// 10.  Poll SSH port 2222 until ready (timeout 10s)
/// 11.  Listen on vsock for MCP_PORT_CHANGE (timeout 2s, fallback 7878)
/// 12.  ShellBridge::write_claude_settings(workspace)
/// 13.  ShellBridge::write_mcp_config(workspace, actual_mcp_port)
/// 14.  SSH: VecReconciler::reconcile() → tombstone chunks for deleted files
/// 15.  IF first boot: SSH: WorkspaceIndexer::index_all()
/// 16.  SSH: BootHook::run() → load META_SEG, write session-context.txt
/// 17.  IF kernel_built_at > 90 days: warn to stderr
/// 18.  Append BOOT event to WITNESS_SEG
/// 19.  exec claude (interactive, in workspace dir)
/// 20.  ON claude exit: ShutdownHook, WITNESS_SEG SHUTDOWN, SessionLock::release,
///      stop Firecracker, kill virtiofsd, rm TAP device, rm temp files
pub async fn run_start(opts: StartOptions) -> anyhow::Result<()> {
    // Step 1: preflight checks.
    crate::preflight::check_dependencies()?;

    // Steps 2–20: wired in Phase 8 once all sub-systems are integrated.
    // See doc-comment above for the full sequence.
    let _ = &opts.rvf;
    let _ = &opts.workspace;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_options_default_workspace_is_current_dir() {
        let opts = StartOptions {
            rvf: PathBuf::from("myapp.rvf"),
            workspace: PathBuf::from("."),
        };
        assert_eq!(opts.workspace, PathBuf::from("."));
    }
}
