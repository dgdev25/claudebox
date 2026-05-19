use std::path::Path;

use crate::commands::context::AppContext;
use crate::commands::output;

pub fn handle_stop_command(rvf: &Path) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    if !cx.paths.pid_path.exists() {
        output::warn("VM is not running (no PID file found).");
    } else {
        claudebox_core::stop::stop_instance(&cx.paths.pid_path)?;
        output::info("VM stopped.");
    }
    Ok(())
}

pub async fn handle_logs_command(
    rvf: &Path,
    follow: bool,
    since: Option<&str>,
) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    let reader = claudebox_firecracker::log_reader::VsockLogReader {
        uds_path: cx.paths.logs_sock_path.clone(),
    };

    if follow {
        reader.follow(&mut tokio::io::stdout()).await?;
        return Ok(());
    }

    let cutoff = if let Some(since_raw) = since {
        chrono::DateTime::parse_from_rfc3339(since_raw)
            .map_err(|e| anyhow::anyhow!("invalid --since timestamp '{since_raw}': {e}"))?
            .with_timezone(&chrono::Utc)
    } else {
        chrono::DateTime::<chrono::Utc>::from(std::time::UNIX_EPOCH)
    };

    let entries = reader.read_since(cutoff).await?;
    for entry in entries {
        println!("{}", entry.pretty_print());
    }
    Ok(())
}

pub fn handle_status_command(rvf: &Path) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    let manifest = &cx.manifest;
    let vm_status = if let Ok(pid) = claudebox_core::stop::read_pid_file(&cx.paths.pid_path) {
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if alive {
            claudebox_core::status::VmStatusDisplay::Running {
                pid,
                ssh_port: manifest.kernel.ssh_port,
                mcp_port: manifest.kernel.mcp_port,
            }
        } else {
            claudebox_core::status::VmStatusDisplay::Stopped
        }
    } else {
        claudebox_core::status::VmStatusDisplay::Stopped
    };

    let rvf_size_mb = std::fs::metadata(rvf)
        .map(|m| m.len() / (1024 * 1024))
        .unwrap_or(0);

    let lang = match &manifest.language {
        claudebox_core::manifest::LanguageProfile::Single(p) => {
            format!("{:?}@{}", p.lang, p.version).to_lowercase()
        }
        claudebox_core::manifest::LanguageProfile::Multi(ps) => ps
            .iter()
            .map(|p| format!("{:?}@{}", p.lang, p.version).to_lowercase())
            .collect::<Vec<_>>()
            .join(", "),
    };

    let info = claudebox_core::status::ProjectStatus {
        project_name: manifest.project_name.clone(),
        rvf_path: cx.paths.rvf.clone(),
        rvf_size_mb,
        vm_status,
        language: lang,
        kernel_age_days: 0,
        kernel_stale: false,
        witness_hot_entries: 0,
        witness_archived_months: 0,
        vec_chunks: 0,
        vec_files: 0,
        vec_tombstoned: 0,
        schema_version: manifest.version,
    };
    println!("{}", claudebox_core::status::format_status(&info));
    Ok(())
}
