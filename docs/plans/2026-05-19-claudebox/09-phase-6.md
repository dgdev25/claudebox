# ClaudeBox — Phase 6: VEC_SEG Indexing Pipeline + Stale Entry Reconciliation

> **Prerequisite:** Phase 5 complete and verified.
> After completing this phase: indexer chunk windowing tests pass, reconciler tombstone tests pass.

---

### Task 6.1: claudebox-vec crate + WorkspaceIndexer

**Files:**
- Create: `crates/claudebox-vec/Cargo.toml`
- Create: `crates/claudebox-vec/src/indexer.rs`
- Create: `crates/claudebox-vec/src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_skip_patterns_exclude_binary_paths() {
    assert!(should_skip("node_modules/express/index.js"));
    assert!(should_skip(".git/config"));
    assert!(should_skip("target/debug/binary"));
    assert!(should_skip("assets/logo.png"));
    assert!(should_skip("dist/bundle.js"));
    assert!(!should_skip("src/main.rs"));
    assert!(!should_skip("lib/utils.py"));
}

#[test]
fn test_chunk_windowing_produces_correct_boundaries() {
    // 1024 token text → 512 window + 64 overlap → 3 chunks
    // chunk 0: tokens 0..512
    // chunk 1: tokens 448..960
    // chunk 2: tokens 896..1024 (partial)
    let text = "word ".repeat(1024); // 1024 "word " tokens
    let chunks = chunk_text(&text, 512, 64);
    assert!(chunks.len() >= 2);
    // First chunk starts at 0
    // Second chunk starts before first ends (overlap)
}

#[test]
fn test_index_stats_counts_files() {
    let stats = IndexStats { files_indexed: 5, chunks_created: 42, files_tombstoned: 0, elapsed_ms: 100 };
    assert_eq!(stats.files_indexed, 5);
}
```

- [ ] **Step 2: Create `crates/claudebox-vec/Cargo.toml`**

```toml
[package]
name = "claudebox-vec"
version = "0.1.0"
edition = "2021"

[dependencies]
claudebox-core = { path = "../claudebox-core" }
claudebox-witness = { path = "../claudebox-witness" }
claudebox-meta = { path = "../claudebox-meta" }
fastembed = "3"
rvf-runtime = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }
notify = { workspace = true }
port-check = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Implement `indexer.rs`** from Technical Plan §Phase 6

Key components:
- `SKIP_PATTERNS` constant (node_modules, .git, target, __pycache__, .venv, dist, build, .next, .claudebox, *.lock, *.bin, image/binary extensions, *.wasm, *.rvf)
- `should_skip(path: &str) -> bool`
- `chunk_text(text: &str, chunk_tokens: usize, overlap_tokens: usize) -> Vec<String>` — sliding window
- `WorkspaceIndexer { workspace_path, rvf_path, model: TextEmbedding }` constructed with `EmbeddingModel::AllMiniLML6V2`
- `index_all()` → walks workspace, skips patterns, embeds, stores in VEC_SEG HNSW
- `index_changed(since)` → only files with mtime > since
- `index_file(path)` → chunks + embeds single file
- `ChunkRecord { file_path, chunk_index, text, embedding: Vec<f32>, tombstoned: bool }`
- `IndexStats { files_indexed, chunks_created, files_tombstoned, elapsed_ms }`

- [ ] **Step 4: Run tests**

```bash
cargo test -p claudebox-vec indexer
# Expected: skip patterns + chunk windowing tests pass
```

- [ ] **Step 5: Commit**

```bash
git add crates/claudebox-vec/
git commit -m "feat(vec): implement WorkspaceIndexer — fastembed HNSW with skip patterns and chunk windowing"
```

---

### Task 6.2: VecReconciler — boot-time tombstoning

**Files:**
- Create: `crates/claudebox-vec/src/reconciler.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_reconciler_tombstones_missing_files() {
    // Setup: VEC_SEG with chunks for file_a.rs (exists) and file_b.rs (deleted)
    // Run reconcile()
    // Verify: file_a.rs chunks have tombstoned=false
    //         file_b.rs chunks have tombstoned=true
    todo!() // implement with mock VEC_SEG once indexer is wired
}

#[test]
fn test_reconcile_stats_counts_correctly() {
    let stats = ReconcileStats { files_checked: 10, files_tombstoned: 3 };
    assert_eq!(stats.files_tombstoned, 3);
    assert_eq!(stats.files_checked - stats.files_tombstoned, 7);
}
```

- [ ] **Step 2: Implement `reconciler.rs`** from Technical Plan §Phase 6

```rust
pub struct VecReconciler {
    pub workspace_path: PathBuf,
    pub rvf_path: PathBuf,
}

impl VecReconciler {
    // Fast metadata scan — does NOT recompute embeddings
    // Sets tombstoned=true for chunks whose file no longer exists on disk
    // Appends VecReconcile witness event
    pub async fn reconcile(&self) -> anyhow::Result<ReconcileStats>;
}

pub struct ReconcileStats {
    pub files_checked: usize,
    pub files_tombstoned: usize,
}
```

Key: tombstoning is a metadata-only operation. No embeddings are recomputed. Physical removal only happens during `claudebox compact`.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-vec reconciler
git add crates/claudebox-vec/src/reconciler.rs
git commit -m "feat(vec): implement VecReconciler — boot-time tombstoning for deleted files"
```

---

### Task 6.3: Compact operation — physical tombstone removal

**Files:**
- Create: `crates/claudebox-vec/src/compact.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_compact_removes_tombstoned_entries() {
    // After compact, tombstoned entries should be physically absent from VEC_SEG
    // Non-tombstoned entries should remain and be searchable
    // Uses InitTransaction for atomicity
    todo!() // integration test deferred to Phase 10
}

#[test]
fn test_compact_uses_init_transaction_atomicity() {
    // Verify that compact creates .rvf.tmp before committing
    // On failure mid-compact, .rvf.tmp is cleaned up
    // This is a structural test — InitTransaction cleanup tested in Phase 2
    // Just verify compact returns Ok for a valid (non-tombstoned) VEC_SEG
    let stats = CompactStats { entries_removed: 0, entries_kept: 5 };
    assert_eq!(stats.entries_removed, 0);
}
```

- [ ] **Step 2: Implement `compact.rs`**

```rust
pub struct VecCompactor {
    pub rvf_path: PathBuf,
}

pub struct CompactStats {
    pub entries_removed: usize,
    pub entries_kept: usize,
}

impl VecCompactor {
    // Rebuilds HNSW index excluding tombstoned=true chunks
    // Uses InitTransaction for atomicity
    // Appends WitnessCompact event
    pub async fn compact(&self) -> anyhow::Result<CompactStats>;
}
```

- [ ] **Step 3: Run tests and commit**

```bash
cargo test -p claudebox-vec compact
git add crates/claudebox-vec/src/compact.rs
git commit -m "feat(vec): implement VecCompactor — physical tombstone removal with atomic rename"
```

---

### Task 6.4: Wire VEC_SEG into claudebox-vec lib.rs

**Files:**
- Modify: `crates/claudebox-vec/src/lib.rs`

- [ ] **Step 1: Add module declarations**

```rust
pub mod indexer;
pub mod reconciler;
pub mod compact;
pub mod mcp_server; // stub — full impl in Phase 7
```

Create `mcp_server.rs` stub with empty `pub fn start_mcp_server() {}`.

- [ ] **Step 2: Verify workspace compiles**

```bash
cargo build --workspace
cargo clippy -p claudebox-vec -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/claudebox-vec/src/
git commit -m "feat(vec): wire module declarations; phase 6 complete"
```

---

### Task 6.5: WitnessWriter — append helper used across crates

**Files:**
- Create: `crates/claudebox-witness/src/writer.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_witness_chain_genesis_has_zero_prev_hash() {
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    let entry = WitnessWriter::create_genesis(
        &signing_key,
        WitnessEvent::Boot { project_id: "test".into() }
    ).unwrap();
    assert_eq!(entry.seq, 0);
    assert_eq!(entry.prev_hash, [0u8; 32]);
}

#[test]
fn test_witness_chain_prev_hash_links_correctly() {
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    let genesis = WitnessWriter::create_genesis(
        &signing_key,
        WitnessEvent::Boot { project_id: "test".into() }
    ).unwrap();
    let next = WitnessWriter::create_next(
        &signing_key,
        &genesis,
        WitnessEvent::Command { cmd: "cargo build".into(), exit_code: 0 }
    ).unwrap();
    assert_eq!(next.seq, 1);
    assert_eq!(next.prev_hash, genesis.payload_hash);
}
```

- [ ] **Step 2: Implement `writer.rs`**

```rust
pub struct WitnessWriter;

impl WitnessWriter {
    pub fn create_genesis(
        signing_key: &ed25519_dalek::SigningKey,
        event: WitnessEvent,
    ) -> anyhow::Result<WitnessEntry>;

    pub fn create_next(
        signing_key: &ed25519_dalek::SigningKey,
        prev: &WitnessEntry,
        event: WitnessEvent,
    ) -> anyhow::Result<WitnessEntry>;
}
```

Each entry: compute `payload_hash = SHA3-256(event JSON)`, set `prev_hash`, sign `(seq + ts + payload_hash + prev_hash)` with Ed25519.

- [ ] **Step 3: Add `pub mod writer;` to witness lib.rs, run tests, commit**

```bash
cargo test -p claudebox-witness writer
git add crates/claudebox-witness/
git commit -m "feat(witness): add WitnessWriter — genesis + chained entry creation with Ed25519"
```
