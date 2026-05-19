use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

#[derive(Parser)]
#[command(name = "claudebox", about = "Per-project isolated Firecracker VM for Claude Code", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Interactive project setup wizard (recommended for most users).
    New {
        /// Optional project name; if omitted, wizard will ask.
        name: Option<String>,
    },
    Init {
        name: String,
        /// Optional language profile(s): `node@22`, `python@3.12`, `rust@1.87`, `go@1.22`.
        /// If omitted, claudebox auto-detects from project files when possible.
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        lang: Vec<String>,
        #[arg(long)]
        allow: Vec<String>,
        #[arg(long)]
        kernel_from: Option<PathBuf>,
    },
    Start {
        rvf: PathBuf,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        /// Run without host workspace mount and expose VM workspace as a local folder via sshfs.
        #[arg(long)]
        isolated: bool,
        /// Local directory to mount VM /workspace into when --isolated is used.
        #[arg(long)]
        mount_dir: Option<PathBuf>,
        /// Optional disk image (.qcow2 or .img) to boot instead of building
        /// an initramfs — required on macOS.
        /// Download: curl -fLO https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-virt-3.21.0-x86_64.iso
        #[arg(long)]
        rootfs: Option<PathBuf>,
    },
    Stop {
        rvf: PathBuf,
    },
    Logs {
        rvf: PathBuf,
        #[arg(long)]
        follow: bool,
        #[arg(long)]
        since: Option<String>,
    },
    Status {
        rvf: PathBuf,
    },
    Branch {
        rvf: PathBuf,
        name: String,
    },
    Rollback {
        rvf: PathBuf,
        branch: String,
    },
    Audit {
        rvf: PathBuf,
        #[arg(long)]
        archive: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Snapshot {
        rvf: PathBuf,
        #[command(subcommand)]
        action: SnapshotAction,
    },
    UpgradeKernel {
        rvf: PathBuf,
        /// Path to the new kernel image (bzImage) to embed.
        /// Defaults to the cached kernel from `claudebox setup`.
        #[arg(long)]
        kernel_from: Option<PathBuf>,
    },
    Kernel {
        rvf: PathBuf,
        #[command(subcommand)]
        action: KernelAction,
    },
    Migrate {
        rvf: PathBuf,
    },
    Compact {
        rvf: PathBuf,
    },
    UpdateAllowlist {
        rvf: PathBuf,
        #[arg(long)]
        add: Vec<String>,
        #[arg(long)]
        remove: Vec<String>,
    },
    Destroy {
        rvf: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Install system dependencies and download default kernel + rootfs.
    Setup {
        /// Re-download/reinstall even if already present.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum SnapshotAction {
    Create {
        name: String,
    },
    Export {
        name: String,
        #[arg(long)]
        output: PathBuf,
    },
    List,
    Restore {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum KernelAction {
    Show,
    Cache,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::New { name } => {
            commands::lifecycle::handle_new_command(name).await?;
        }
        Commands::Init {
            name,
            lang,
            allow,
            kernel_from,
        } => commands::lifecycle::handle_init_command(name, lang, allow, kernel_from).await?,
        Commands::Start { rvf, workspace, isolated, mount_dir, rootfs } =>
            commands::lifecycle::handle_start_command(rvf, workspace, isolated, mount_dir, rootfs).await?,
        Commands::Stop { rvf } => {
            commands::runtime::handle_stop_command(&rvf)?;
        }
        Commands::Logs { rvf, follow, since } =>
            commands::runtime::handle_logs_command(&rvf, follow, since.as_deref()).await?,
        Commands::Status { rvf } => {
            commands::runtime::handle_status_command(&rvf)?;
        }
        Commands::Branch { rvf, name } => {
            commands::storage::handle_branch_command(&rvf, &name)?;
        }
        Commands::Rollback { rvf, branch } => {
            commands::storage::handle_rollback_command(&rvf, &branch)?;
        }
        Commands::Audit { rvf, archive, json } => {
            commands::maintenance::handle_audit_command(&rvf, archive, json)?;
        }
        Commands::Snapshot { rvf, action } => {
            commands::storage::handle_snapshot_command(&rvf, action)?;
        }
        Commands::UpgradeKernel { rvf, kernel_from } => {
            commands::lifecycle::handle_upgrade_kernel_command(&rvf, kernel_from).await?;
        }
        Commands::Kernel { rvf, action } => {
            commands::lifecycle::handle_kernel_command(&rvf, action)?;
        }
        Commands::Migrate { rvf } => {
            commands::maintenance::handle_migrate_command(&rvf)?;
        }
        Commands::Compact { rvf } => {
            commands::storage::handle_compact_command(&rvf)?;
        }
        Commands::UpdateAllowlist { rvf, add, remove } => {
            commands::maintenance::handle_update_allowlist_command(&rvf, add, remove).await?;
        }
        Commands::Destroy { rvf, force } => {
            commands::maintenance::handle_destroy_command(&rvf, force)?;
        }
        Commands::Setup { force } => {
            commands::maintenance::handle_setup_command(force)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_and_migrate_called_before_start() {
        // Type-check test: verifies check_and_migrate has the right signature.
        // If the import or signature is wrong, this won't compile.
        let _: fn(&std::path::Path, bool) -> anyhow::Result<()> =
            claudebox_migrate::check_and_migrate;
    }

    #[test]
    fn test_init_subcommand_parses_single_lang() {
        let cli = Cli::try_parse_from(["claudebox", "init", "myapp", "--lang", "node@22"]).unwrap();
        match cli.command {
            Commands::Init { name, lang, .. } => {
                assert_eq!(name, "myapp");
                assert_eq!(lang, vec!["node@22"]);
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_new_subcommand_parses_optional_name() {
        let cli = Cli::try_parse_from(["claudebox", "new", "myapp"]).unwrap();
        match cli.command {
            Commands::New { name } => assert_eq!(name.as_deref(), Some("myapp")),
            _ => panic!("Expected New"),
        }
    }

    #[test]
    fn test_init_subcommand_parses_multi_lang() {
        let cli = Cli::try_parse_from([
            "claudebox",
            "init",
            "polyglot",
            "--lang",
            "node@22,rust@1.87",
        ])
        .unwrap();
        match cli.command {
            Commands::Init { lang, .. } => {
                assert_eq!(lang.len(), 2);
                assert!(lang.contains(&"node@22".to_string()));
                assert!(lang.contains(&"rust@1.87".to_string()));
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_start_subcommand_default_workspace() {
        let cli = Cli::try_parse_from(["claudebox", "start", "myapp.rvf"]).unwrap();
        match cli.command {
            Commands::Start { workspace, .. } => {
                assert_eq!(workspace, PathBuf::from("."));
            }
            _ => panic!("Expected Start"),
        }
    }

    #[test]
    fn test_audit_subcommand_with_archive_flag() {
        let cli = Cli::try_parse_from([
            "claudebox",
            "audit",
            "myapp.rvf",
            "--archive",
            "2026-04",
        ])
        .unwrap();
        match cli.command {
            Commands::Audit { archive, .. } => {
                assert_eq!(archive.unwrap(), "2026-04");
            }
            _ => panic!("Expected Audit"),
        }
    }
}
