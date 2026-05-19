use std::path::Path;

const PRE_COMMIT_SCRIPT: &str = r#"#!/bin/bash
# ClaudeBox pre-commit hook
if [ -f "$(git rev-parse --show-toplevel)/.claudebox/session.lock" ]; then
    echo "Warning: ClaudeBox session is active. Consider stopping the session before committing." >&2
fi
exit 0
"#;

/// Write (or overwrite) the ClaudeBox git pre-commit hook in `workspace/.git/hooks/`.
///
/// Creates the hooks directory if it does not exist. The hook warns the developer
/// when a ClaudeBox session is active at commit time but does not block the commit.
pub fn write_pre_commit_hook(workspace: &Path) -> anyhow::Result<()> {
    let hooks_dir = workspace.join(".git/hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| anyhow::anyhow!("failed to create .git/hooks: {e}"))?;

    let hook_path = hooks_dir.join("pre-commit");
    std::fs::write(&hook_path, PRE_COMMIT_SCRIPT)
        .map_err(|e| anyhow::anyhow!("failed to write pre-commit hook: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| anyhow::anyhow!("failed to set pre-commit hook permissions: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_pre_commit_hook_is_executable() {
        let dir = tempdir().unwrap();
        // Create fake .git/hooks dir.
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        write_pre_commit_hook(dir.path()).unwrap();
        let hook_path = dir.path().join(".git/hooks/pre-commit");
        assert!(hook_path.exists());
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&hook_path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn test_pre_commit_hook_contains_session_lock_check() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
        write_pre_commit_hook(dir.path()).unwrap();
        let hook_path = dir.path().join(".git/hooks/pre-commit");
        let content = std::fs::read_to_string(hook_path).unwrap();
        assert!(content.contains("session.lock"));
        assert!(content.contains("#!/bin/bash"));
    }
}
