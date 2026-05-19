use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "claudebox", about = "Per-project isolated Firecracker VM for Claude Code")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Init {
        name: String,
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
        Commands::Init {
            name,
            lang,
            allow,
            kernel_from,
        } => {
            claudebox_core::init::run_init(
                claudebox_core::init::InitOptions {
                    name,
                    lang,
                    allow,
                    kernel_from,
                },
                &std::env::current_dir()?,
            )
            .await?;
        }
        Commands::Start { rvf, workspace } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            claudebox_core::start::run_start(claudebox_core::start::StartOptions {
                rvf,
                workspace,
            })
            .await?;
        }
        Commands::Stop { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("stop command not yet fully implemented")
        }
        Commands::Logs { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("logs command not yet fully implemented")
        }
        Commands::Status { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("status command not yet fully implemented")
        }
        Commands::Branch { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("branch command not yet fully implemented")
        }
        Commands::Rollback { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("rollback command not yet fully implemented")
        }
        Commands::Audit { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("audit command not yet fully implemented")
        }
        Commands::Snapshot { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("snapshot command not yet fully implemented")
        }
        Commands::UpgradeKernel { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("upgrade-kernel command not yet fully implemented")
        }
        Commands::Kernel { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("kernel command not yet fully implemented")
        }
        Commands::Migrate { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("migrate command not yet fully implemented")
        }
        Commands::Compact { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("compact command not yet fully implemented")
        }
        Commands::UpdateAllowlist { rvf, add, remove } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            claudebox_core::allowlist::run_update_allowlist(&rvf, add, remove).await?;
        }
        Commands::Destroy { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("destroy command not yet fully implemented")
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
