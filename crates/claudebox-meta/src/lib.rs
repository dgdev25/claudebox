pub mod hooks;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub last_boot: Option<String>,
    pub working_dir: String,
    pub open_files: Vec<String>,
    pub task_context: String,
    pub scratchpad: String,
    pub history: Vec<HistoryEntry>,
    pub installed_packages: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts: String,
    pub kind: HistoryEntryKind,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum HistoryEntryKind {
    Command,
    FileWrite,
    FileDelete,
    PackageInstall,
    Note,
}

impl SessionState {
    pub fn to_claude_prompt_prefix(&self) -> String {
        format!(
            "## ClaudeBox Session Context\n\
             Last session: {last}\n\
             Working directory: {wd}\n\
             Current task: {task}\n\
             Notes from last session:\n{notes}\n\
             Recent history (last 20 entries):\n{history}",
            last = self.last_boot.as_deref().unwrap_or("first boot"),
            wd = self.working_dir,
            task = self.task_context,
            notes = self.scratchpad,
            history = self
                .history
                .iter()
                .rev()
                .take(20)
                .map(|e| format!("  - [{}] {}", e.ts, e.value))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_prompt_prefix() {
        let state = SessionState {
            last_boot: Some("2026-05-19T09:00:00Z".into()),
            working_dir: "/workspace/src".into(),
            task_context: "Implementing JWT".into(),
            scratchpad: "Use axum extractor".into(),
            history: vec![HistoryEntry {
                ts: "2026-05-19T09:00:01Z".into(),
                kind: HistoryEntryKind::Command,
                value: "cargo build".into(),
            }],
            installed_packages: vec!["axum@0.7".into()],
            open_files: vec!["src/main.rs".into()],
        };
        let prefix = state.to_claude_prompt_prefix();
        assert!(prefix.contains("ClaudeBox Session Context"));
        assert!(prefix.contains("Implementing JWT"));
        assert!(prefix.contains("cargo build"));
    }

    #[test]
    fn test_session_state_default() {
        let state = SessionState::default();
        assert!(state.last_boot.is_none());
        assert!(state.history.is_empty());
    }
}
