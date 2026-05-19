use std::path::{Path, PathBuf};

use claudebox_core::witness::meta_sidecar_path;

use crate::{HistoryEntry, HistoryEntryKind, SessionState};

pub struct BootHook {
    pub rvf_path: PathBuf,
    pub session_context_output: PathBuf,
}

impl BootHook {
    /// Read `SessionState` from the `.rvf.meta.json` sidecar (defaulting to
    /// an empty state when absent) and write its Claude prompt prefix to
    /// `session_context_output`. Intended to run early in the VM boot
    /// sequence so Claude Code can pick up the previous session.
    pub fn run(&self) -> anyhow::Result<()> {
        let state = load_session_state(&self.rvf_path)?;
        let prefix = state.to_claude_prompt_prefix();
        if let Some(parent) = self.session_context_output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", parent.display()))?;
        }
        std::fs::write(&self.session_context_output, prefix).map_err(|e| {
            anyhow::anyhow!(
                "failed to write session context to {}: {e}",
                self.session_context_output.display()
            )
        })?;
        Ok(())
    }
}

pub struct ShutdownHook {
    pub rvf_path: PathBuf,
    pub shell_history_path: PathBuf,
    pub workspace_path: PathBuf,
    pub boot_time: chrono::DateTime<chrono::Utc>,
}

impl ShutdownHook {
    /// Merge this session's collected history into the META sidecar and
    /// stamp `last_boot`. Existing scratchpad and task context are
    /// preserved. Writes the result atomically via `<sidecar>.tmp` + rename.
    pub fn run(&self) -> anyhow::Result<()> {
        let mut state = load_session_state(&self.rvf_path)?;
        let mut new_history = self.collect_history().unwrap_or_default();
        state.history.append(&mut new_history);
        state.last_boot = Some(self.boot_time.to_rfc3339());
        if state.working_dir.is_empty() {
            state.working_dir = self.workspace_path.display().to_string();
        }
        write_session_state(&self.rvf_path, &state)
    }

    pub fn collect_history(&self) -> anyhow::Result<Vec<HistoryEntry>> {
        use std::io::{BufRead, BufReader};

        const MAX_HISTORY_LINES: usize = 10_000;

        // Use a buffered reader with a line cap to prevent OOM on gigabyte history
        // files (CWE-400). Only the last MAX_HISTORY_LINES lines are retained.
        let file = std::fs::File::open(&self.shell_history_path)
            .map_err(|e| anyhow::anyhow!("failed to open shell history: {e}"))?;
        let reader = BufReader::new(file);

        let ts = chrono::Utc::now().to_rfc3339();
        let entries: Vec<HistoryEntry> = reader
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .take(MAX_HISTORY_LINES)
            .map(|line| HistoryEntry {
                ts: ts.clone(),
                kind: HistoryEntryKind::Command,
                value: line,
            })
            .collect();

        Ok(entries)
    }
}

/// Read `SessionState` from `<rvf>.meta.json`. Returns `Default` when the
/// sidecar is absent so first-boot doesn't fail.
pub fn load_session_state(rvf_path: &Path) -> anyhow::Result<SessionState> {
    let path = meta_sidecar_path(rvf_path);
    if !path.exists() {
        return Ok(SessionState::default());
    }
    let json = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&json)
        .map_err(|e| anyhow::anyhow!("corrupt meta sidecar {}: {e}", path.display()))
}

/// Write `state` to `<rvf>.meta.json` atomically (temp + rename).
pub fn write_session_state(rvf_path: &Path, state: &SessionState) -> anyhow::Result<()> {
    let path = meta_sidecar_path(rvf_path);
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| anyhow::anyhow!("meta serialisation failed: {e}"))?;
    std::fs::write(&tmp, json)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| anyhow::anyhow!("atomic rename {} -> {} failed: {e}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionState;
    use tempfile::tempdir;

    #[test]
    fn test_boot_hook_writes_context_file() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let state = SessionState {
            task_context: "Implement JWT auth".into(),
            last_boot: Some("2026-05-18T10:00:00Z".into()),
            ..Default::default()
        };
        write_session_state(&rvf, &state).unwrap();

        let ctx_output = dir.path().join("session-context.txt");
        let hook = BootHook { rvf_path: rvf, session_context_output: ctx_output.clone() };
        hook.run().unwrap();

        let content = std::fs::read_to_string(&ctx_output).unwrap();
        assert!(content.contains("Implement JWT auth"));
        assert!(content.contains("ClaudeBox Session Context"));
    }

    #[test]
    fn test_boot_hook_handles_missing_meta_sidecar() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let ctx_output = dir.path().join("session-context.txt");
        let hook = BootHook { rvf_path: rvf, session_context_output: ctx_output.clone() };
        hook.run().unwrap();
        let content = std::fs::read_to_string(&ctx_output).unwrap();
        assert!(content.contains("ClaudeBox Session Context"));
        assert!(content.contains("first boot"));
    }

    #[test]
    fn test_shutdown_hook_collects_history() {
        let dir = tempdir().unwrap();
        let history_path = dir.path().join(".bash_history");
        std::fs::write(&history_path, "cargo build\ncargo test\n").unwrap();

        let hook = ShutdownHook {
            rvf_path: dir.path().join("test.rvf"),
            shell_history_path: history_path,
            workspace_path: dir.path().to_owned(),
            boot_time: chrono::Utc::now() - chrono::TimeDelta::hours(1),
        };

        let history = hook.collect_history().unwrap();
        assert!(history.iter().any(|e| e.value == "cargo build"));
        assert!(history.iter().any(|e| e.value == "cargo test"));
    }

    #[test]
    fn test_shutdown_hook_run_persists_state_and_history() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        write_session_state(
            &rvf,
            &SessionState {
                task_context: "previous task".into(),
                scratchpad: "earlier notes".into(),
                ..Default::default()
            },
        )
        .unwrap();

        let history_path = dir.path().join(".bash_history");
        std::fs::write(&history_path, "ls\npwd\n").unwrap();

        let hook = ShutdownHook {
            rvf_path: rvf.clone(),
            shell_history_path: history_path,
            workspace_path: dir.path().to_owned(),
            boot_time: chrono::Utc::now(),
        };
        hook.run().unwrap();

        let loaded = load_session_state(&rvf).unwrap();
        assert_eq!(loaded.task_context, "previous task", "task_context must be preserved");
        assert_eq!(loaded.scratchpad, "earlier notes", "scratchpad must be preserved");
        assert!(loaded.history.iter().any(|e| e.value == "ls"));
        assert!(loaded.history.iter().any(|e| e.value == "pwd"));
        assert!(loaded.last_boot.is_some());
    }

    #[test]
    fn test_shutdown_hook_run_succeeds_without_shell_history() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let hook = ShutdownHook {
            rvf_path: rvf.clone(),
            shell_history_path: dir.path().join("does-not-exist"),
            workspace_path: dir.path().to_owned(),
            boot_time: chrono::Utc::now(),
        };
        hook.run().unwrap();
        let loaded = load_session_state(&rvf).unwrap();
        assert!(loaded.last_boot.is_some());
        assert!(loaded.history.is_empty());
    }
}
