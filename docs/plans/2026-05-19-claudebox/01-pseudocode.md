# ClaudeBox — Pseudocode

## `claudebox init` Flow

```
1. Parse args: name, lang(s), allow domains, kernel_from (optional)
2. Run preflight: check firecracker, virtiofsd, clang, rvf-cli, ssh exist
3. Build ClaudeBoxManifest:
   - project_id = UUID v4
   - language = Single(profile) or Multi([profiles])
   - network = NetworkPolicy::for_profiles(profiles)  // domain union, dedup
   - resources = ResourceLimits::default()
   - kernel = KernelConfig { ssh_port: 2222, mcp_port: 7878, arch: detect }
   - witness = WitnessPolicy::default()
4. Generate Ed25519 keypair → write ~/.claudebox/keys/<project_id>.key (0o600)
5. Start InitTransaction(<name>, cwd):  // .rvf.tmp, Drop cleans on failure
6.   Build kernel:
     IF --kernel-from provided: KernelImporter::import_from_rvf → use cached
     ELSE IF cached (cache_key matches): use cached
     ELSE: KernelBuilder::build() → runs build.sh via Docker
7.   Compile eBPF:
     IF macOS: generate squid config
     ELSE: EbpfCompiler::compile_to_bytes() → bytecode
8.   ApplianceBuilder::build_skeleton(tmp_path):
     - Write MANIFEST_SEG
     - Write empty META_SEG (SessionState::default())
     - Write empty VEC_SEG + INDEX_SEG
     - Write CRYPTO_SEG (Ed25519 pubkey)
     - Write genesis WITNESS_SEG entry
9.   ApplianceBuilder::embed_kernel(store, kernel_path)
10.  ApplianceBuilder::embed_ebpf(store, ebpf_bytes)
11.  Sign entire appliance → update CRYPTO_SEG
12.  Write .git/hooks/pre-commit (warn if session.lock exists)
13. InitTransaction::commit() → rename .rvf.tmp → <name>.rvf
```

## `claudebox start` Flow

```
1. preflight::check_dependencies()
2. Verify CRYPTO_SEG signature → error if tampered
3. check_and_migrate(rvf_path) → auto-migrate minor versions
4. SessionLock::check() → error if live PID, clear if stale PID
5. WitnessCompactor::compact_if_needed() → archive if >10K entries or >30 days
6. Extract KERNEL_SEG to /tmp/claudebox-<project_id>/bzImage
7. Start virtiofsd sidecar: socket=/tmp/claudebox-<id>.virtiofs, dir=workspace
8. Configure Firecracker VM via HTTP API on socket /tmp/claudebox-<id>.sock:
   PUT /boot-source, /drives/rootfs, /machine-config, /network-interfaces/eth0
   PUT /vsock { guest_cid: 3, uds_path: /tmp/claudebox-<id>.vsock }
   PUT /actions { action_type: "InstanceStart" }
9. SessionLock::acquire(workspace, { pid, vm_id, started_at, ssh_port })
10. Poll SSH port 2222 until ready (timeout 10s)
11. Listen on vsock for MCP_PORT_CHANGE (timeout 2s, fallback 7878)
12. ShellBridge::write_claude_settings(workspace)   // .claude/settings.json
13. ShellBridge::write_mcp_config(workspace, actual_mcp_port)  // .claude/mcp.json
14. SSH: VecReconciler::reconcile() → tombstone chunks for deleted files
15. IF first boot: SSH: WorkspaceIndexer::index_all()
16. SSH: BootHook::run() → load META_SEG, write /run/claudebox/session-context.txt
17. IF kernel_built_at > 90 days: print warning to stderr
18. Append BOOT event to WITNESS_SEG
19. exec claude (interactive, in workspace dir)
20. ON claude exit:
    SSH: ShutdownHook::run() → serialise SessionState → write META_SEG
    Append SHUTDOWN event to WITNESS_SEG
    SessionLock::release()
    Stop Firecracker, kill virtiofsd, rm TAP device, rm temp files
```

## SSH Shell Bridge

```
~/.claudebox/bin/claudebox-shell:
  exec ssh -i $CLAUDEBOX_KEY_PATH -p $CLAUDEBOX_SSH_PORT \
    -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o ConnectTimeout=5 claude@127.0.0.1 "$@"

.claude/settings.json:
  { "shell": "<bridge-script>", "env": { KEY_PATH, SSH_PORT } }

.claude/mcp.json:
  { "mcpServers": { "claudebox": { ssh tunnel → VM :7878 } } }
```

## WITNESS_SEG Chain

```
entry[n] = WitnessEntry {
  seq:          n,
  ts_nanos:     now(),
  event:        WitnessEvent::Boot { project_id },
  payload_hash: SHA3-256(event payload),
  prev_hash:    entry[n-1].payload_hash,  // genesis = [0u8;32]
  signature:    Ed25519(seq + ts + payload_hash + prev_hash),
}
```

## VEC_SEG Indexing

```
index_all:
  FOR file IN walk(workspace), SKIP skip_patterns:
    chunks = sliding_window(file, size=512, overlap=64)
    embeddings = model.embed(chunks)  // all-MiniLM-L6-v2, 384-dim
    HNSW.insert(embeddings, metadata={path, chunk_index, text, tombstoned=false})

reconcile (on boot):
  FOR path IN unique_paths(VEC_SEG):
    IF NOT file_exists(workspace / path):
      mark all chunks for path: tombstoned=true
  append VecReconcile { files_tombstoned } to WITNESS_SEG

compact:
  rebuild HNSW excluding tombstoned=true chunks  // via InitTransaction
  append WitnessCompact event
```

## WitnessCompactor

```
compact_if_needed:
  entries = read_all(WITNESS_SEG)
  IF len(entries) > max_entries OR oldest_entry.age > retention_days:
    cutoff = entries[-max_entries] or entries within retention window
    archive = entries[:cutoff]  → write to ~/.claudebox/archive/<project>/<YYYY-MM>.rvf
    keep    = entries[cutoff:]  → rewrite WITNESS_SEG
    append WitnessCompact { entries_archived, archive_path } to new chain
```

## MCP Server Tools

```
search_codebase(query, k=5):
  q_embedding = model.embed(query)
  results = HNSW.search(q_embedding, k)
  FILTER results WHERE NOT tombstoned
  RETURN top-k { file_path, chunk_index, text, score }

get_session_context():
  RETURN SessionState { last_boot, working_dir, task_context, scratchpad, history[-20:] }
```

## Data Flow: Claude Code → VM

```
[developer's machine]
Claude Code CLI
  ↓ bash tool call: "cargo build"
claudebox-shell (SSH wrapper)
  ↓ SSH -p 2222
[Firecracker VM]
  bash executor → runs "cargo build" in /workspace
  output → SSH stdout → Claude Code sees result

[developer's machine]
Claude Code CLI
  ↓ MCP tool: search_codebase("JWT middleware")
SSH tunnel → localhost:7878 → VM :7878
  rvf-mcp-server → HNSW query → VEC_SEG
  → returns top-k code chunks
```

## Error Cases

```
init failure mid-way:
  InitTransaction::drop → removes .rvf.tmp → zero artifacts on disk

start while session running (live PID):
  error: "ClaudeBox session already active (PID X). Stop with: claudebox stop myproject.rvf"

start with stale lock (dead PID):
  clear lock automatically → proceed with boot

eBPF domain not resolvable:
  error: "Cannot resolve registry.npmjs.org — check network before init"

VM boot timeout (SSH not ready in 10s):
  stop firecracker → kill virtiofsd → remove session lock → remove temp files
  error: "VM boot timed out after 10s. Check /tmp/claudebox-<id>.sock for Firecracker logs."
```
