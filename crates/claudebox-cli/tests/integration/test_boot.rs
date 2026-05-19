#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_boot_and_session_restore() {
    // claudebox init → claudebox start → wait for SSH
    // Write session data, claudebox stop
    // claudebox start again
    // Verify session-context.txt contains previous task_context
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_session_lock_prevents_double_boot() {
    // Start VM
    // Attempt claudebox start on same .rvf
    // Verify error contains "already running"
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_stale_lock_cleared_on_boot() {
    // Write lock file with non-existent PID
    // Attempt claudebox start
    // Verify boot succeeds (stale lock auto-cleared)
}

#[test]
#[ignore = "requires KVM + Firecracker"]
fn test_mcp_port_conflict_resolved() {
    // Start VM; artificially bind 7878 before MCP server starts
    // Verify MCP server binds on 7879
    // Verify .claude/mcp.json updated with 7879
}
