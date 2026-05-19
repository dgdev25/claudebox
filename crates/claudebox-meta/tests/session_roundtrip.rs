use claudebox_meta::{HistoryEntry, HistoryEntryKind, SessionState};

#[test]
fn test_session_state_serialise_roundtrip() {
    let original = SessionState {
        last_boot: Some("2026-05-19T09:00:00Z".into()),
        working_dir: "/workspace/src".into(),
        task_context: "Implementing JWT middleware".into(),
        scratchpad: "Use axum extractor pattern".into(),
        history: vec![
            HistoryEntry {
                ts: "2026-05-19T09:00:01Z".into(),
                kind: HistoryEntryKind::Command,
                value: "cargo build".into(),
            },
            HistoryEntry {
                ts: "2026-05-19T09:00:15Z".into(),
                kind: HistoryEntryKind::FileWrite,
                value: "src/auth.rs".into(),
            },
        ],
        installed_packages: vec!["axum@0.7".into(), "tokio@1.38".into()],
        open_files: vec!["src/main.rs".into()],
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: SessionState = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.task_context, original.task_context);
    assert_eq!(restored.history.len(), 2);
    assert_eq!(restored.installed_packages, original.installed_packages);

    // Verify prompt prefix includes restored context
    let prefix = restored.to_claude_prompt_prefix();
    assert!(prefix.contains("Implementing JWT middleware"));
    assert!(prefix.contains("cargo build"));
}
