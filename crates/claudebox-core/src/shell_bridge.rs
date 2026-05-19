use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct ShellBridge {
    pub project_id: String,
    pub ssh_port: u16,
    pub key_path: PathBuf,
    pub mcp_port: u16,
}

impl ShellBridge {
    /// Writes `~/.claudebox/bin/claudebox-shell` with permissions 0o755.
    pub fn install_shell_script(&self) -> Result<()> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        let bin_dir = home.join(".claudebox").join("bin");
        std::fs::create_dir_all(&bin_dir)?;

        let script_path = bin_dir.join("claudebox-shell");
        let script = "#!/usr/bin/env bash\n\
            exec ssh \\\n  \
            -i \"${CLAUDEBOX_KEY_PATH}\" \\\n  \
            -p \"${CLAUDEBOX_SSH_PORT}\" \\\n  \
            -o StrictHostKeyChecking=no \\\n  \
            -o UserKnownHostsFile=/dev/null \\\n  \
            -o ConnectTimeout=5 \\\n  \
            claude@127.0.0.1 \"$@\"\n";

        std::fs::write(&script_path, script)?;

        // Set executable permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        Ok(())
    }

    /// Writes `.claude/settings.json` into the given workspace directory.
    /// Redirects Claude Code's bash tool through the SSH bridge.
    pub fn write_claude_settings(&self, workspace: &Path) -> Result<()> {
        let claude_dir = workspace.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        let shell_path = home.join(".claudebox").join("bin").join("claudebox-shell");
        let key_path_str = self.key_path.to_string_lossy();

        let settings = serde_json::json!({
            "shell": shell_path.to_string_lossy(),
            "env": {
                "CLAUDEBOX_KEY_PATH": key_path_str,
                "CLAUDEBOX_SSH_PORT": self.ssh_port.to_string()
            }
        });

        let content = serde_json::to_string_pretty(&settings)?;
        std::fs::write(claude_dir.join("settings.json"), content)?;
        Ok(())
    }

    /// Writes `.claude/mcp.json` into the given workspace directory.
    /// Configures the ClaudeBox MCP server tools for Claude Code.
    /// Uses `actual_mcp_port` rather than `self.mcp_port` — allows runtime port override.
    pub fn write_mcp_config(&self, workspace: &Path, actual_mcp_port: u16) -> Result<()> {
        let claude_dir = workspace.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        let key_path_str = self.key_path.to_string_lossy();
        let ssh_port_str = self.ssh_port.to_string();
        let forward_spec = format!("{}:localhost:{}", actual_mcp_port, actual_mcp_port);

        let config = serde_json::json!({
            "mcpServers": {
                "claudebox": {
                    "command": "ssh",
                    "args": [
                        "-i", key_path_str,
                        "-p", ssh_port_str,
                        "-L", forward_spec,
                        "-o", "StrictHostKeyChecking=no",
                        "claude@127.0.0.1",
                        "claudebox-mcp"
                    ],
                    "transport": "stdio"
                }
            }
        });

        let content = serde_json::to_string_pretty(&config)?;
        std::fs::write(claude_dir.join("mcp.json"), content)?;
        Ok(())
    }
}

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
        let script_path = dirs::home_dir().unwrap().join(".claudebox/bin/claudebox-shell");
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
