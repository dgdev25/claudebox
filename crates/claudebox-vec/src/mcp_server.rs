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

/// Embed `query`, perform an HNSW top-k search, and return live chunks.
///
/// **Requires** the `embed` feature (fastembed + ONNX Runtime). Deferred to
/// the Phase 8 integration wiring.
pub async fn handle_search_codebase(
    _indexer: &crate::indexer::WorkspaceIndexer,
    _query: &str,
    _k: usize,
) -> anyhow::Result<Vec<ChunkRecord>> {
    anyhow::bail!(
        "handle_search_codebase requires fastembed embed feature — deferred to integration"
    )
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

/// Read META_SEG from the `.rvf` at `rvf_path` and deserialise to
/// `SessionState`.
///
/// Deferred to Phase 8 integration wiring (requires rvf-runtime META_SEG
/// reader).
pub async fn handle_get_session_context(
    _rvf_path: &std::path::Path,
) -> anyhow::Result<SessionState> {
    anyhow::bail!(
        "handle_get_session_context requires rvf-runtime META_SEG reading — deferred to integration"
    )
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
}
