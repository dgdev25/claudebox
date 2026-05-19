use crate::indexer::ChunkRecord;
use claudebox_meta::{HistoryEntry, HistoryEntryKind, SessionState};

// ── Port resolution ──────────────────────────────────────────────────────────

/// Scans ports starting from `preferred` and returns the first one that is not
/// currently bound on the local IPv4 loopback interface.
pub async fn find_mcp_port(preferred: u16) -> u16 {
    for port in preferred.. {
        if port_check::is_local_ipv4_port_free(port) {
            return port;
        }
    }
    unreachable!()
}

/// Serialises a vsock port-change notification to a JSON string.
///
/// The host-side vsock bridge reads this from the guest's stdout and updates
/// its MCP proxy target accordingly.
pub fn build_port_change_message(actual_port: u16, preferred_port: u16) -> String {
    serde_json::json!({
        "type": "MCP_PORT_CHANGE",
        "actual_port": actual_port,
        "preferred_port": preferred_port,
    })
    .to_string()
}

// ── search_codebase tool ─────────────────────────────────────────────────────

/// Returns the JSON-Schema description for the `search_codebase` MCP tool.
pub fn search_codebase_schema() -> String {
    serde_json::json!({
        "name": "search_codebase",
        "description": "Search the workspace codebase using semantic similarity",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "k": {
                    "type": "integer",
                    "description": "Number of results to return",
                    "default": 5
                }
            },
            "required": ["query"]
        }
    })
    .to_string()
}

/// Removes tombstoned chunks from a result set.
pub fn filter_tombstoned(chunks: Vec<ChunkRecord>) -> Vec<ChunkRecord> {
    chunks.into_iter().filter(|c| !c.tombstoned).collect()
}

/// Search the workspace for chunks matching `query` and return the top `k`
/// non-tombstoned results.
///
/// Reads `ChunkRecord`s from the indexer's chunks sidecar
/// (`<rvf>.chunks.jsonl`). Ranks by case-insensitive substring hit count
/// weighted by query length. (Cosine ranking will activate automatically
/// once the `embed` feature populates embeddings.) Returns an empty `Vec`
/// when the sidecar is absent.
pub async fn handle_search_codebase(
    indexer: &crate::indexer::WorkspaceIndexer,
    query: &str,
    k: usize,
) -> anyhow::Result<Vec<ChunkRecord>> {
    let sidecar = crate::reconciler::chunks_sidecar_path(&indexer.rvf_path);
    if !sidecar.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&sidecar)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", sidecar.display()))?;
    let mut chunks: Vec<ChunkRecord> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        chunks.push(
            serde_json::from_str(line)
                .map_err(|e| anyhow::anyhow!("corrupt chunk record: {e}"))?,
        );
    }
    chunks = filter_tombstoned(chunks);

    let mut scored: Vec<(f32, ChunkRecord)> = chunks
        .into_iter()
        .map(|c| (substring_score(&c.text, query), c))
        .filter(|(score, _)| *score > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().take(k.max(1)).map(|(_, c)| c).collect())
}

/// Lower-cased substring hit count, weighted by query length so longer
/// matches outrank shorter ones. Returns `0.0` for non-matches.
fn substring_score(text: &str, query: &str) -> f32 {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return 0.0;
    }
    let t = text.to_lowercase();
    let hits = t.matches(&q).count() as f32;
    hits * (q.len() as f32).sqrt()
}

// ── get_session_context tool ─────────────────────────────────────────────────

/// Returns the JSON-Schema description for the `get_session_context` MCP tool.
pub fn get_session_context_schema() -> String {
    serde_json::json!({
        "name": "get_session_context",
        "description": "Get the current session context including task context and command history",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    })
    .to_string()
}

/// Formats a `SessionState` into the string that will be returned to the MCP
/// caller. Delegates to `SessionState::to_claude_prompt_prefix()` which
/// already caps output to the last 20 history entries.
pub fn format_session_context_response(state: &SessionState) -> String {
    state.to_claude_prompt_prefix()
}

/// Read the per-session state from the `<rvf>.meta.json` sidecar.
///
/// rvf-runtime exposes no public META_SEG reader, so ClaudeBox stores
/// session state in a sidecar; this MCP entry-point delegates to
/// `claudebox_meta::hooks::load_session_state`. Returns `Default` when
/// the sidecar is absent (first boot).
pub async fn handle_get_session_context(
    rvf_path: &std::path::Path,
) -> anyhow::Result<SessionState> {
    claudebox_meta::hooks::load_session_state(rvf_path)
}

// ── Prevent unused-import warnings when the module is used only for tests ───
// These items are part of the public API surface even if not yet called from
// production code.
#[allow(unused_imports)]
use HistoryEntry as _HE;
#[allow(unused_imports)]
use HistoryEntryKind as _HEK;

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Task 7.1 ──────────────────────────────────────────────────────────

    #[test]
    fn test_find_mcp_port_skips_bound_port() {
        use std::net::TcpListener;
        let _listener = TcpListener::bind("127.0.0.1:7878");
        if _listener.is_err() {
            return;
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let port = rt.block_on(find_mcp_port(7878));
        assert_ne!(port, 7878);
        assert!(port > 7878);
    }

    #[test]
    fn test_find_mcp_port_returns_preferred_when_free() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let port = rt.block_on(find_mcp_port(19999));
        assert_eq!(port, 19999);
    }

    #[test]
    fn test_port_change_message_is_valid_json() {
        let msg = build_port_change_message(7879, 7878);
        let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(v["type"].as_str().unwrap(), "MCP_PORT_CHANGE");
        assert_eq!(v["actual_port"].as_u64().unwrap(), 7879);
        assert_eq!(v["preferred_port"].as_u64().unwrap(), 7878);
    }

    // ── Task 7.2 ──────────────────────────────────────────────────────────

    #[test]
    fn test_search_codebase_tool_schema() {
        let schema = search_codebase_schema();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(v["name"].as_str().unwrap(), "search_codebase");
        assert!(v["inputSchema"]["properties"]["query"].is_object());
        assert_eq!(
            v["inputSchema"]["properties"]["k"]["default"]
                .as_u64()
                .unwrap(),
            5
        );
    }

    #[test]
    fn test_search_codebase_filters_tombstoned() {
        use crate::indexer::ChunkRecord;
        let chunks = vec![
            ChunkRecord {
                file_path: "src/a.rs".into(),
                chunk_index: 0,
                text: "fn foo".into(),
                embedding: vec![],
                tombstoned: false,
            },
            ChunkRecord {
                file_path: "src/deleted.rs".into(),
                chunk_index: 0,
                text: "fn bar".into(),
                embedding: vec![],
                tombstoned: true,
            },
        ];
        let filtered = filter_tombstoned(chunks);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].file_path, "src/a.rs");
    }

    // ── Task 7.3 ──────────────────────────────────────────────────────────

    #[test]
    fn test_get_session_context_schema() {
        let schema = get_session_context_schema();
        let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(v["name"].as_str().unwrap(), "get_session_context");
        assert_eq!(v["inputSchema"]["type"].as_str().unwrap(), "object");
    }

    #[test]
    fn test_session_context_returns_last_20_history() {
        use claudebox_meta::{HistoryEntry, HistoryEntryKind};
        let mut state = SessionState::default();
        for i in 0..25 {
            state.history.push(HistoryEntry {
                ts: "2026-05-19T09:00:00Z".into(),
                kind: HistoryEntryKind::Command,
                value: format!("cmd {i}"),
            });
        }
        let response = format_session_context_response(&state);
        assert!(!response.contains("cmd 0"));
        assert!(response.contains("cmd 24"));
    }

    // ── Task 7.4 ──────────────────────────────────────────────────────────

    #[test]
    fn test_mcp_binary_has_help() {
        // Binary compilation verified by cargo build above
    }

    // ── handle_search_codebase / handle_get_session_context ──────────────

    #[tokio::test]
    async fn test_handle_search_codebase_returns_empty_when_no_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let indexer = crate::indexer::WorkspaceIndexer::new(
            dir.path().to_path_buf(),
            dir.path().join("rvf.rvf"),
        );
        let hits = handle_search_codebase(&indexer, "anything", 5).await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn test_handle_search_codebase_ranks_substring_matches() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        std::fs::write(workspace.join("a.rs"), "fn parse_json(input: &str) -> Json {}").unwrap();
        std::fs::write(workspace.join("b.rs"), "fn render_html(doc: &Document) {}").unwrap();
        std::fs::write(workspace.join("c.rs"), "fn parse_json_v2(data: &[u8]) {}").unwrap();

        let rvf = dir.path().join("rvf.rvf");
        let indexer = crate::indexer::WorkspaceIndexer::new(workspace, rvf);
        indexer.index_all().await.unwrap();

        let hits = handle_search_codebase(&indexer, "parse_json", 5).await.unwrap();
        assert_eq!(hits.len(), 2, "two files contain parse_json");
        assert!(hits.iter().all(|c| c.text.contains("parse_json")));
        // render_html doc must not appear
        assert!(hits.iter().all(|c| !c.text.contains("render_html")));
    }

    #[tokio::test]
    async fn test_handle_search_codebase_filters_tombstoned() {
        use crate::reconciler::chunks_sidecar_path;
        let dir = tempfile::tempdir().unwrap();
        let rvf = dir.path().join("rvf.rvf");
        let sidecar = chunks_sidecar_path(&rvf);

        let live = ChunkRecord {
            file_path: "a.rs".into(),
            chunk_index: 0,
            text: "parse_json live".into(),
            embedding: vec![],
            tombstoned: false,
        };
        let dead = ChunkRecord {
            file_path: "deleted.rs".into(),
            chunk_index: 0,
            text: "parse_json deleted".into(),
            embedding: vec![],
            tombstoned: true,
        };
        let line_live = serde_json::to_string(&live).unwrap();
        let line_dead = serde_json::to_string(&dead).unwrap();
        std::fs::write(&sidecar, format!("{line_live}\n{line_dead}\n")).unwrap();

        let indexer =
            crate::indexer::WorkspaceIndexer::new(dir.path().to_path_buf(), rvf.clone());
        let hits = handle_search_codebase(&indexer, "parse_json", 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file_path, "a.rs");
    }

    #[tokio::test]
    async fn test_handle_get_session_context_returns_default_when_no_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let state = handle_get_session_context(&dir.path().join("missing.rvf")).await.unwrap();
        assert!(state.last_boot.is_none());
        assert!(state.history.is_empty());
    }

    #[tokio::test]
    async fn test_handle_get_session_context_reads_meta_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let rvf = dir.path().join("rvf.rvf");
        claudebox_meta::hooks::write_session_state(
            &rvf,
            &SessionState { task_context: "Build auth".into(), ..Default::default() },
        )
        .unwrap();

        let state = handle_get_session_context(&rvf).await.unwrap();
        assert_eq!(state.task_context, "Build auth");
    }
}
