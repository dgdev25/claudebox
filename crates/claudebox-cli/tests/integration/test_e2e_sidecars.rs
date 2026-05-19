//! End-to-end sidecar pipeline test.
//!
//! Exercises the tier 3/4/5 stubs landed in this branch via public Rust
//! APIs — no KVM, no Docker, no rvf-cli. Builds a minimal `.rvf` from
//! scratch, then drives:
//!
//! 1. Genesis witness write (#13)
//! 2. Allowlist update + witness append (#14)
//! 3. Witness compaction (#15)
//! 4. Audit (chain integrity + table/JSON format) (#16)
//! 5. Kernel upgrade with manifest preservation (#17)
//! 6. Kernel import to a cache dir (#19)
//! 7. Workspace indexing → chunks sidecar (#22)
//! 8. MCP substring search (#26)
//! 9. VEC reconcile tombstoning (#20)
//! 10. VEC compact removing tombstones (#21)
//! 11. Boot / Shutdown hook META sidecar round-trip (#25)
//! 12. Vsock log reader pretty-print over a local UDS (#23)

use claudebox_core::manifest::{
    ClaudeBoxManifest, KernelConfig, Lang, LanguageProfile, NetworkPolicy, ResourceLimits,
    SingleProfile,
};
use claudebox_core::witness::{
    append_witness_entry, load_witness_entries, manifest_sidecar_path, meta_sidecar_path,
    signing_key_path_for_rvf, witness_path_for_rvf,
};
use claudebox_meta::SessionState;
use claudebox_meta::hooks::{BootHook, ShutdownHook, load_session_state, write_session_state};
use claudebox_rvf::builder::ApplianceBuilder;
use claudebox_rvf::kernel_import::KernelImporter;
use claudebox_rvf::kernel_upgrade::KernelUpgrader;
use claudebox_vec::indexer::WorkspaceIndexer;
use claudebox_vec::mcp_server::{handle_get_session_context, handle_search_codebase};
use claudebox_vec::reconciler::{VecReconciler, chunks_sidecar_path};
use claudebox_witness::audit::{format_audit_json, format_audit_table, verify_chain};
use claudebox_witness::compaction::{WitnessCompactor, WitnessPolicy};
use claudebox_witness::{WitnessEntry, WitnessEvent};
use tempfile::tempdir;

fn make_manifest() -> ClaudeBoxManifest {
    ClaudeBoxManifest {
        version: 1,
        project_id: "e2e-project".into(),
        project_name: "e2e".into(),
        language: LanguageProfile::Single(SingleProfile {
            lang: Lang::Node,
            version: "22".into(),
        }),
        created_at: "2026-05-19T00:00:00Z".into(),
        kernel_built_at: "2026-05-19T00:00:00Z".into(),
        network: NetworkPolicy {
            allow_domains: vec!["registry.npmjs.org".into()],
            allow_localhost: true,
            dns_server: "1.1.1.1".into(),
        },
        resources: ResourceLimits::default(),
        kernel: KernelConfig {
            arch: "x86_64".into(),
            ssh_port: 2222,
            mcp_port: 7878,
        },
        witness: claudebox_core::manifest::WitnessPolicy::default(),
    }
}

#[tokio::test]
async fn end_to_end_sidecar_lifecycle() {
    let dir = tempdir().unwrap();
    let rvf = dir.path().join("e2e.rvf");

    // ── 1. Build .rvf + genesis witness ──────────────────────────────────
    let builder = ApplianceBuilder::new(make_manifest()).unwrap();
    builder.build_skeleton(&rvf, None).unwrap();
    builder.write_genesis_witness(&rvf).unwrap();
    let witness_file = witness_path_for_rvf(&rvf);
    let key_file = signing_key_path_for_rvf(&rvf);
    assert!(witness_file.exists(), "genesis witness file must exist");
    assert!(key_file.exists(), "signing key must be persisted");
    let entries = load_witness_entries(&rvf).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].seq, 0);
    assert_eq!(entries[0].prev_hash, [0u8; 32]);

    // ── 2. Allowlist update → live manifest sidecar + AllowlistUpdate evt
    claudebox_core::allowlist::run_update_allowlist(
        &rvf,
        vec!["github.com".into()],
        vec![],
    )
    .await
    .unwrap();
    assert!(manifest_sidecar_path(&rvf).exists());
    let entries = load_witness_entries(&rvf).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[1].event,
        WitnessEvent::AllowlistUpdate { .. }
    ));

    // ── 3. Pad the witness chain, then compact ──────────────────────────
    for i in 0..30 {
        append_witness_entry(
            &rvf,
            WitnessEvent::Command { cmd: format!("cmd-{i}"), exit_code: 0 },
        )
        .unwrap();
    }
    assert_eq!(load_witness_entries(&rvf).unwrap().len(), 32);

    let compactor = WitnessCompactor {
        rvf_path: rvf.clone(),
        policy: WitnessPolicy { max_entries: 10, retention_days: 30 },
        archive_dir: dir.path().join("archives"),
    };
    let result = compactor.compact_if_needed().unwrap();
    assert_eq!(result.entries_kept, 10);
    assert_eq!(result.entries_archived, 22);
    let archive_path = result.archive_path.expect("archive file must exist");
    assert!(archive_path.exists());

    // ── 4. Audit table + JSON format + chain verify on kept entries ─────
    let kept: Vec<WitnessEntry> = load_witness_entries(&rvf).unwrap();
    assert_eq!(kept.len(), 10);
    let table = format_audit_table(&kept);
    assert!(table.contains("Chain integrity"));
    assert!(table.contains("COMMAND"));
    let json = format_audit_json(&kept).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["chain_valid"].as_bool().unwrap_or(false) || v["broken_at_seq"].is_number());

    // ── 5. Kernel upgrade preserving manifest (cmdline) ─────────────────
    let new_kernel = dir.path().join("new-bzImage.bin");
    std::fs::write(&new_kernel, b"NEW-KERNEL-PAYLOAD").unwrap();
    let upgrader = KernelUpgrader { rvf_path: rvf.clone() };
    let upgrade = upgrader.upgrade(&new_kernel).await.unwrap();
    assert_ne!(upgrade.from_hash, upgrade.to_hash);

    // Manifest sidecar (live state) and witness sidecar survive the rename:
    assert!(manifest_sidecar_path(&rvf).exists());
    let entries_after_upgrade = load_witness_entries(&rvf).unwrap();
    assert!(matches!(
        entries_after_upgrade.last().unwrap().event,
        WitnessEvent::KernelUpgrade { .. }
    ));

    // ── 6. Import kernel from .rvf into a sibling cache dir ──────────────
    let cache_dir = dir.path().join("imported-cache");
    let imported = KernelImporter::import_from_rvf_to(&rvf, &cache_dir)
        .await
        .unwrap();
    assert_eq!(imported, cache_dir.join("kernel"));
    assert_eq!(std::fs::read(&imported).unwrap(), b"NEW-KERNEL-PAYLOAD");

    // ── 7. Workspace indexer → chunks sidecar ────────────────────────────
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::write(
        workspace.join("src/main.rs"),
        "fn handle_request() { parse_json() }",
    )
    .unwrap();
    std::fs::write(
        workspace.join("src/util.rs"),
        "fn format_response() { render_html() }",
    )
    .unwrap();

    let indexer = WorkspaceIndexer::new(workspace.clone(), rvf.clone());
    let stats = indexer.index_all().await.unwrap();
    assert_eq!(stats.files_indexed, 2);
    assert!(stats.chunks_created >= 2);
    let sidecar = chunks_sidecar_path(&rvf);
    assert!(sidecar.exists());

    // ── 8. MCP substring search ──────────────────────────────────────────
    let hits = handle_search_codebase(&indexer, "parse_json", 5).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].file_path, "src/main.rs");
    let multi = handle_search_codebase(&indexer, "fn ", 10).await.unwrap();
    assert!(multi.len() >= 2, "two source files both contain `fn `");

    // ── 9. VEC reconcile: delete util.rs, reconcile, expect tombstone ───
    std::fs::remove_file(workspace.join("src/util.rs")).unwrap();
    let reconciler = VecReconciler { workspace_path: workspace.clone(), rvf_path: rvf.clone() };
    let r_stats = reconciler.reconcile().await.unwrap();
    assert_eq!(r_stats.files_tombstoned, 1);
    let entries_after_reconcile = load_witness_entries(&rvf).unwrap();
    assert!(entries_after_reconcile.iter().any(|e| matches!(
        e.event,
        WitnessEvent::VecReconcile { files_removed: 1 }
    )));

    // Search must now skip the tombstoned file's chunk:
    let post_recon = handle_search_codebase(&indexer, "render_html", 5).await.unwrap();
    assert!(post_recon.is_empty(), "tombstoned chunk must be filtered out");

    // ── 10. VEC compact: physically remove tombstoned chunks ────────────
    let vc = claudebox_vec::compact::VecCompactor { rvf_path: rvf.clone() };
    let c_stats = vc.compact().await.unwrap();
    assert_eq!(c_stats.entries_removed, 1);
    let raw = std::fs::read_to_string(&sidecar).unwrap();
    assert!(!raw.contains("util.rs"), "util.rs chunk must be physically gone");

    // ── 11. Boot/Shutdown hook META sidecar round-trip ───────────────────
    write_session_state(
        &rvf,
        &SessionState {
            task_context: "ship the e2e test".into(),
            scratchpad: "remember: cargo fmt".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let ctx_out = dir.path().join("session-context.txt");
    BootHook { rvf_path: rvf.clone(), session_context_output: ctx_out.clone() }
        .run()
        .unwrap();
    let prefix = std::fs::read_to_string(&ctx_out).unwrap();
    assert!(prefix.contains("ship the e2e test"));

    // Shutdown hook appends history without clobbering scratchpad/task_context.
    let history_file = dir.path().join(".bash_history");
    std::fs::write(&history_file, "cargo test\ncargo build\n").unwrap();
    ShutdownHook {
        rvf_path: rvf.clone(),
        shell_history_path: history_file,
        workspace_path: workspace.clone(),
        boot_time: chrono::Utc::now(),
    }
    .run()
    .unwrap();
    let post_shutdown: SessionState = load_session_state(&rvf).unwrap();
    assert_eq!(post_shutdown.task_context, "ship the e2e test");
    assert_eq!(post_shutdown.scratchpad, "remember: cargo fmt");
    assert!(post_shutdown.history.iter().any(|h| h.value == "cargo test"));
    assert!(post_shutdown.last_boot.is_some());
    assert!(meta_sidecar_path(&rvf).exists());

    // MCP session context returns the merged state:
    let mcp_session = handle_get_session_context(&rvf).await.unwrap();
    assert_eq!(mcp_session.task_context, "ship the e2e test");

    // ── 12. Chain verification: hashes still link after all the churn ───
    let final_entries = load_witness_entries(&rvf).unwrap();
    let verify = verify_chain(&final_entries, None);
    assert!(verify.is_valid, "witness chain must remain hash-valid end-to-end");
}

#[tokio::test]
async fn end_to_end_vsock_log_streaming() {
    use claudebox_firecracker::log_reader::{LogEntry, VsockLogReader};
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

    let dir = tempdir().unwrap();
    let socket = dir.path().join("logs.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    // One-shot vsock-equivalent UDS server that feeds two log lines and
    // closes — mirrors what claudebox-logd would write from inside a VM.
    let payload = b"{\"ts\":\"2026-05-19T09:00:00Z\",\"kind\":\"CMD\",\"data\":{\"cmd\":\"npm install\"}}\n\
                    {\"ts\":\"2026-05-19T10:00:00Z\",\"kind\":\"NET\",\"data\":{\"host\":\"registry.npmjs.org\"}}\n";
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let _ = sock.write_all(payload).await;
            let _ = sock.shutdown().await;
        }
    });

    let reader = VsockLogReader { uds_path: socket.clone() };
    let cutoff = chrono::DateTime::parse_from_rfc3339("2026-05-19T09:30:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let new_entries: Vec<LogEntry> = reader.read_since(cutoff).await.unwrap();
    assert_eq!(new_entries.len(), 1);
    assert_eq!(new_entries[0].kind, "NET");

    // Second connection: full follow stream pretty-prints both entries.
    let listener2 = UnixListener::bind(dir.path().join("logs2.sock")).unwrap();
    let socket2 = dir.path().join("logs2.sock");
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener2.accept().await {
            let _ = sock.write_all(payload).await;
            let _ = sock.shutdown().await;
        }
    });
    let reader2 = VsockLogReader { uds_path: socket2 };
    let mut out: Vec<u8> = Vec::new();
    reader2.follow(&mut out).await.unwrap();
    let pretty = String::from_utf8(out).unwrap();
    assert!(pretty.contains("09:00:00") && pretty.contains("npm install"));
    assert!(pretty.contains("10:00:00") && pretty.contains("registry.npmjs.org"));
}
