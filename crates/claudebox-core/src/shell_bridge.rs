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
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        self.install_shell_script_to(&home)
    }

    /// Writes `{base}/.claudebox/bin/claudebox-shell` with permissions 0o755.
    /// The `~/.claudebox/` directory is created with permissions 0o700.
    /// Accepts a `base` directory override — used for test isolation.
    pub fn install_shell_script_to(&self, base: &Path) -> Result<()> {
        let claudebox_dir = base.join(".claudebox");
        let bin_dir = claudebox_dir.join("bin");
        std::fs::create_dir_all(&bin_dir)?;

        // Restrict ~/.claudebox/ to owner only
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claudebox_dir, Permissions::from_mode(0o700))?;
        }

        let script_path = bin_dir.join("claudebox-shell");
        let script = "#!/usr/bin/env bash\n\
            exec ssh \\\n  \
            -i \"${CLAUDEBOX_KEY_PATH}\" \\\n  \
            -p \"${CLAUDEBOX_SSH_PORT}\" \\\n  \
            -o StrictHostKeyChecking=accept-new \\\n  \
            -o UserKnownHostsFile=\"${HOME}/.claudebox/known_hosts\" \\\n  \
            -o ConnectTimeout=5 \\\n  \
            claude@127.0.0.1 \"$@\"\n";

        std::fs::write(&script_path, script)?;

        // Set executable permissions (Unix only)
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, Permissions::from_mode(0o755))?;
        }

        Ok(())
    }

    /// Writes `.claude/settings.json` into the given workspace directory.
    /// Redirects Claude Code's bash tool through the SSH bridge.
    /// The `.claude/` directory is created with permissions 0o700.
    /// The settings file is written with permissions 0o600.
    pub fn write_claude_settings(&self, workspace: &Path) -> Result<()> {
        let claude_dir = workspace.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        // Restrict .claude/ directory to owner only
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude_dir, Permissions::from_mode(0o700))?;
        }

        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
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
        let settings_path = claude_dir.join("settings.json");
        std::fs::write(&settings_path, content)?;

        // Restrict settings.json to owner read-write only
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&settings_path, Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Writes `.claude/mcp.json` into the given workspace directory.
    /// Configures the ClaudeBox MCP server tools for Claude Code.
    /// Uses `actual_mcp_port` rather than `self.mcp_port` — allows runtime port override.
    /// The `.claude/` directory is created with permissions 0o700.
    /// The config file is written with permissions 0o600.
    pub fn write_mcp_config(&self, workspace: &Path, actual_mcp_port: u16) -> Result<()> {
        let claude_dir = workspace.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        // Restrict .claude/ directory to owner only
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude_dir, Permissions::from_mode(0o700))?;
        }

        let key_path_str = self.key_path.to_string_lossy();
        let ssh_port_str = self.ssh_port.to_string();
        let forward_spec = format!("127.0.0.1:{}:localhost:{}", actual_mcp_port, actual_mcp_port);

        let config = serde_json::json!({
            "mcpServers": {
                "claudebox": {
                    "command": "ssh",
                    "args": [
                        "-i", key_path_str,
                        "-p", ssh_port_str,
                        "-L", forward_spec,
                        "-o", "StrictHostKeyChecking=accept-new",
                        "-o", "UserKnownHostsFile=~/.claudebox/known_hosts",
                        "claude@127.0.0.1",
                        "claudebox-mcp"
                    ],
                    "transport": "stdio"
                }
            }
        });

        let content = serde_json::to_string_pretty(&config)?;
        let mcp_path = claude_dir.join("mcp.json");
        std::fs::write(&mcp_path, content)?;

        // Restrict mcp.json to owner read-write only
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mcp_path, Permissions::from_mode(0o600))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_bridge() -> ShellBridge {
        ShellBridge {
            project_id: "test-id".into(),
            ssh_port: 2222,
            key_path: PathBuf::from("/tmp/test.key"),
            mcp_port: 7878,
        }
    }

    #[test]
    fn test_install_shell_script_mode() {
        let base = tempdir().unwrap();
        let bridge = make_bridge();
        bridge.install_shell_script_to(base.path()).unwrap();
        let script_path = base.path().join(".claudebox/bin/claudebox-shell");
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
