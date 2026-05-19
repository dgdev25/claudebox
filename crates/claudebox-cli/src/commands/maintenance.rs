use std::path::Path;

use crate::commands::context::AppContext;
use crate::commands::output;

pub fn handle_audit_command(
    rvf: &Path,
    archive: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    let entries = if let Some(month) = archive {
        let paths = claudebox_witness::compaction::find_archives_for_month(
            &cx.paths.witness_archive_dir,
            &month,
        )?;
        anyhow::ensure!(
            !paths.is_empty(),
            "no archive files found for month {month} in {}",
            cx.paths.witness_archive_dir.display()
        );
        let mut all = Vec::new();
        for path in &paths {
            let mut e = claudebox_witness::read_jsonl_entries(path)?;
            all.append(&mut e);
        }
        all.sort_by_key(|e| e.seq);
        all
    } else {
        claudebox_core::witness::load_witness_entries(rvf)?
    };
    if json {
        println!("{}", claudebox_witness::audit::format_audit_json(&entries)?);
    } else {
        println!("{}", claudebox_witness::audit::format_audit_table(&entries));
    }
    Ok(())
}

pub fn handle_migrate_command(rvf: &Path) -> anyhow::Result<()> {
    let schema_version = claudebox_migrate::read_schema_version(rvf).unwrap_or(1);
    let chain = claudebox_migrate::MigrationChain::new();
    let latest = chain.latest_version();
    if schema_version == latest {
        output::info(&format!(
            "Already at latest schema version (v{latest}), nothing to do."
        ));
    } else if schema_version > latest {
        anyhow::bail!(
            "{} schema v{schema_version} is newer than this binary (max v{latest}); \
             upgrade claudebox",
            rvf.display()
        );
    } else {
        output::action(&format!(
            "Migrating {} from v{schema_version} to v{latest}",
            rvf.display()
        ));
        let new_version = chain.migrate_to_latest(rvf, schema_version)?;
        output::info(&format!("Migration complete — now at v{new_version}."));
    }
    Ok(())
}

pub async fn handle_update_allowlist_command(
    rvf: &Path,
    add: Vec<String>,
    remove: Vec<String>,
) -> anyhow::Result<()> {
    let _ = AppContext::for_rvf(rvf)?;
    claudebox_core::allowlist::run_update_allowlist(rvf, add, remove).await?;
    Ok(())
}

pub fn handle_destroy_command(rvf: &Path, force: bool) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    claudebox_core::stop::destroy_instance(&cx.manifest.project_id, force)?;
    output::info(&format!("VM data for '{}' destroyed.", cx.manifest.project_name));
    Ok(())
}

pub fn handle_setup_command(force: bool) -> anyhow::Result<()> {
    claudebox_core::setup::run_setup(force)
}
