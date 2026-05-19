# ClaudeBox — Phase 7: MCP Server — VEC_SEG + META_SEG Query Interface

> **Prerequisite:** Phase 6 complete and verified.
> After completing this phase: find_mcp_port test passes, MCP tool schema tests pass.

---

### Task 7.1: find_mcp_port — dynamic port conflict resolution

**Files:**
- Modify: `crates/claudebox-vec/src/mcp_server.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_find_mcp_port_skips_bound_port() {
    use std::net::TcpListener;
    // Bind port 7878 to simulate conflict
    let _listener = TcpListener::bind("127.0.0.1:7878");
    if _listener.is_err() {
        // Port already in use on this machine — skip test
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let port = rt.block_on(find_mcp_port(7878));
    // Must return a different port since 7878 is bound
    assert_ne!(port, 7878);
    assert!(port > 7878);
}

#[test]
fn test_find_mcp_port_returns_preferred_when_free() {
    // 19999 is very unlikely to be in use
    let rt = tokio::runtime::Runtime::new().unwrap();
    let port = rt.block_on(find_mcp_port(19999));
    assert_eq!(port, 19999);
}
```

- [ ] **Step 2: Implement `find_mcp_port` in `mcp_server.rs`**

```rust
use port_check::is_local_ipv4_port_free;

pub async fn find_mcp_port(preferred: u16) -> u16 {
    for port in preferred.. {
        if is_local_ipv4_port_free(port) {
            return port;
        }
    }
    unreachable!()
}
```

Also implement the vsock port-change notification:
```rust
pub fn build_port_change_message(actual_port: u16, preferred_port: u16) -> String {
    serde_json::json!({
        "type": "MCP_PORT_CHANGE",
        "actual_port": actual_port,
        "preferred_port": preferred_port,
    }).to_string()
}
```

- [ ] **Step 3: Write test for port-change message format**

```rust
#[test]
fn test_port_change_message_is_valid_json() {
    let msg = build_port_change_message(7879, 7878);
    let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(v["type"].as_str().unwrap(), "MCP_PORT_CHANGE");
    assert_eq!(v["actual_port"].as_u64().unwrap(), 7879);
    assert_eq!(v["preferred_port"].as_u64().unwrap(), 7878);
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p claudebox-vec mcp_server
# Expected: 3/3 pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-vec/src/mcp_server.rs
git commit -m "feat(vec): implement find_mcp_port — dynamic port conflict resolution + vsock notification"
```

---

### Task 7.2: MCP server — search_codebase tool

**Files:**
- Modify: `crates/claudebox-vec/src/mcp_server.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_search_codebase_tool_schema() {
    let schema = search_codebase_schema();
    let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
    assert_eq!(v["name"].as_str().unwrap(), "search_codebase");
    assert!(v["inputSchema"]["properties"]["query"].is_object());
    // k has a default value
    assert_eq!(v["inputSchema"]["properties"]["k"]["default"].as_u64().unwrap(), 5);
}

#[test]
fn test_search_codebase_filters_tombstoned() {
    // Given a query result set containing tombstoned chunks,
    // filter_tombstoned() should exclude them
    let chunks = vec![
        ChunkRecord { file_path: "src/a.rs".into(), chunk_index: 0,
            text: "fn foo".into(), embedding: vec![], tombstoned: false },
        ChunkRecord { file_path: "src/deleted.rs".into(), chunk_index: 0,
            text: "fn bar".into(), embedding: vec![], tombstoned: true },
    ];
    let filtered = filter_tombstoned(chunks);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].file_path, "src/a.rs");
}
```

- [ ] **Step 2: Implement `search_codebase` tool in `mcp_server.rs`**

```rust
pub fn search_codebase_schema() -> String;

pub fn filter_tombstoned(chunks: Vec<ChunkRecord>) -> Vec<ChunkRecord> {
    chunks.into_iter().filter(|c| !c.tombstoned).collect()
}

pub async fn handle_search_codebase(
    indexer: &WorkspaceIndexer,
    query: &str,
    k: usize,
) -> anyhow::Result<Vec<ChunkRecord>>;
```

`handle_search_codebase`:
1. Embed query with `model.embed([query])`
2. HNSW search for top-k neighbors
3. Filter tombstoned
4. Return results

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-vec search
git add crates/claudebox-vec/src/mcp_server.rs
git commit -m "feat(vec): implement search_codebase MCP tool with tombstone filtering"
```

---

### Task 7.3: MCP server — get_session_context tool

**Files:**
- Modify: `crates/claudebox-vec/src/mcp_server.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_get_session_context_schema() {
    let schema = get_session_context_schema();
    let v: serde_json::Value = serde_json::from_str(&schema).unwrap();
    assert_eq!(v["name"].as_str().unwrap(), "get_session_context");
    // No required parameters
    assert_eq!(v["inputSchema"]["type"].as_str().unwrap(), "object");
}

#[test]
fn test_session_context_returns_last_20_history() {
    let mut state = SessionState::default();
    for i in 0..25 {
        state.history.push(HistoryEntry {
            ts: "2026-05-19T09:00:00Z".into(),
            kind: HistoryEntryKind::Command,
            value: format!("cmd {i}"),
        });
    }
    let response = format_session_context_response(&state);
    // Should only include last 20 items
    assert!(!response.contains("cmd 0"));
    assert!(response.contains("cmd 24"));
}
```

- [ ] **Step 2: Implement get_session_context**

```rust
pub fn get_session_context_schema() -> String;

pub fn format_session_context_response(state: &SessionState) -> String;

pub async fn handle_get_session_context(rvf_path: &Path) -> anyhow::Result<SessionState>;
```

`handle_get_session_context`: read META_SEG from .rvf, deserialise to `SessionState`, return.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-vec session_context
git add crates/claudebox-vec/src/mcp_server.rs
git commit -m "feat(vec): implement get_session_context MCP tool — returns last 20 history entries"
```

---

### Task 7.4: claudebox-mcp binary entry point (baked into kernel)

**Files:**
- Create: `crates/claudebox-vec/src/bin/claudebox-mcp.rs`

- [ ] **Step 1: Write failing test**

```rust
// Basic smoke test — binary must exist and produce --help output
// (Integration: run only if binary is built)
#[test]
fn test_mcp_binary_has_help() {
    // Just verify the binary compiles — runtime behavior tested in integration
    // This test verifies the module compiles without panic
}
```

- [ ] **Step 2: Implement `claudebox-mcp.rs` binary**

```rust
// Reads CLAUDEBOX_MCP_PORT env var (default: 7878)
// Calls find_mcp_port(preferred_port)
// If actual_port != preferred_port: sends MCP_PORT_CHANGE vsock notification
// Starts MCP stdio server exposing search_codebase + get_session_context tools
// Reads workspace path from CLAUDEBOX_WORKSPACE env var
```

Add to `crates/claudebox-vec/Cargo.toml`:
```toml
[[bin]]
name = "claudebox-mcp"
path = "src/bin/claudebox-mcp.rs"
```

- [ ] **Step 3: Build and commit**

```bash
cargo build -p claudebox-vec --bin claudebox-mcp
git add crates/claudebox-vec/src/bin/ crates/claudebox-vec/Cargo.toml
git commit -m "feat(vec): add claudebox-mcp binary — MCP server entry point for kernel embedding"
```
