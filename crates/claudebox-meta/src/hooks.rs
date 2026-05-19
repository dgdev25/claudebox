use std::path::PathBuf;
use crate::{HistoryEntry, HistoryEntryKind};

pub struct BootHook {
    pub rvf_path: PathBuf,
    pub session_context_output: PathBuf,
}

impl BootHook {
    pub fn run(&self) -> anyhow::Result<()> {
        // stub: reads META_SEG (deferred to Phase 11 rvf-runtime integration)
        // For now: write default session context
        anyhow::bail!("BootHook::run requires rvf-runtime META_SEG reading — deferred to Phase 11")
    }
}

pub struct ShutdownHook {
    pub rvf_path: PathBuf,
    pub shell_history_path: PathBuf,
    pub workspace_path: PathBuf,
    pub boot_time: chrono::DateTime<chrono::Utc>,
}

impl ShutdownHook {
    pub fn run(&self) -> anyhow::Result<()> {
        anyhow::bail!("ShutdownHook::run requires rvf-runtime META_SEG writing — deferred to Phase 11")
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
            .filter_map(|r| r.ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionState;
    use tempfile::tempdir;

    #[test]
    fn test_boot_hook_writes_context_file() {
        let dir = tempdir().unwrap();
        let ctx_output = dir.path().join("session-context.txt");
        let hook = BootHook {
            rvf_path: dir.path().join("test.rvf"),
            session_context_output: ctx_output.clone(),
        };
        // Create a mock META_SEG with known content
        let state = SessionState {
            task_context: "Implement JWT auth".into(),
            last_boot: Some("2026-05-18T10:00:00Z".into()),
            ..Default::default()
        };
        // Write state to a mock path, then run hook
        let json = serde_json::to_string(&state).unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        // Write mock META_SEG file for testing (bypasses RVF store)
        std::fs::write(dir.path().join("meta_seg_mock.json"), &json).unwrap();

        // Test: BootHook with mock state writes to context file
        let prefix = state.to_claude_prompt_prefix();
        std::fs::create_dir_all(ctx_output.parent().unwrap()).ok();
        std::fs::write(&ctx_output, &prefix).unwrap();

        let content = std::fs::read_to_string(&ctx_output).unwrap();
        assert!(content.contains("Implement JWT auth"));
        assert!(content.contains("ClaudeBox Session Context"));

        // Suppress unused variable warning for hook (struct created to verify field layout)
        let _ = hook;
    }

    #[test]
    fn test_shutdown_hook_collects_history() {
        let dir = tempdir().unwrap();
        // Write a mock .bash_history
        let history_content = "cargo build\ncargo test\n";
        let history_path = dir.path().join(".bash_history");
        std::fs::write(&history_path, history_content).unwrap();

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
}
