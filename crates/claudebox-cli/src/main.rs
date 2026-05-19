use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "claudebox", about = "Per-project isolated Firecracker VM for Claude Code", version)]
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
        Commands::Init {
            name,
            lang,
            allow,
            kernel_from,
        } => {
            // Auto-detect kernel from setup data dir when flag not supplied.
            let kernel_from = kernel_from.or_else(|| {
                let default = claudebox_core::setup::default_kernel_path();
                if default.exists() { Some(default) } else { None }
            });

            let output_dir = std::env::current_dir()?;
            let manifest = claudebox_core::init::run_init(
                claudebox_core::init::InitOptions {
                    name,
                    lang,
                    allow,
                    kernel_from: kernel_from.clone(),
                },
                &output_dir,
            )
            .await?;

            let output_path = output_dir.join(format!("{}.rvf", manifest.project_name));
            anyhow::ensure!(
                !output_path.exists(),
                "{} already exists — remove it before re-initialising",
                output_path.display()
            );

            let builder = claudebox_rvf::builder::ApplianceBuilder::new(manifest)?;
            builder.build_skeleton(&output_path, kernel_from.as_deref())?;

            eprintln!("Created appliance: {}", output_path.display());
            if kernel_from.is_none() {
                eprintln!(
                    "Note: no kernel embedded. Build one with Docker or supply \
                     --kernel-from <bzImage> to enable `claudebox start`."
                );
            }

            git_commit_rvf(&output_path, &output_dir);
        }
        Commands::Start { rvf, workspace, rootfs } => {
            // Auto-detect rootfs from setup data dir on macOS when flag not supplied.
            let rootfs = rootfs.or_else(|| {
                #[cfg(target_os = "macos")]
                {
                    let default = claudebox_core::setup::default_rootfs_path();
                    if default.exists() { return Some(default); }
                }
                None
            });

            claudebox_migrate::check_and_migrate(&rvf, true)?;
            let opts = claudebox_core::start::StartOptions {
                rvf: rvf.clone(),
                workspace: workspace.clone(),
            };
            let extracted = claudebox_core::start::extract_kernel(&opts)?;

            // Try to build initramfs; on macOS this will fail — caller uses --rootfs instead
            let tmp_dir = extracted.kernel_path.parent().unwrap().to_path_buf();
            let initramfs_path = claudebox_firecracker::initramfs::build_initramfs(&tmp_dir).ok();

            if initramfs_path.is_none() && rootfs.is_none() {
                anyhow::bail!(
                    "No initramfs could be built and no --rootfs supplied.\n\
                     macOS: claudebox start <rvf> --rootfs <alpine.qcow2>\n\
                     Download Alpine: curl -fLO https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-virt-3.21.0-x86_64.iso"
                );
            }

            // Resolve workspace to an absolute path so QEMU receives a stable path.
            let workspace_abs = workspace.canonicalize().unwrap_or(workspace.clone());

            // Build and spawn QEMU
            let mut qemu_cmd = claudebox_firecracker::qemu::build_qemu_command(
                &extracted.kernel_path,
                initramfs_path.as_deref(),
                extracted.ssh_port,
                512,
                rootfs.as_deref(),
                Some(&workspace_abs),
            )?;

            eprintln!(
                "Launching QEMU for {} (ssh_port={})…",
                rvf.display(),
                extracted.ssh_port
            );

            let status = qemu_cmd
                .spawn()
                .map_err(|e| anyhow::anyhow!("failed to spawn QEMU: {e}"))?
                .wait()
                .map_err(|e| anyhow::anyhow!("QEMU wait failed: {e}"))?;

            if !status.success() {
                anyhow::bail!("QEMU exited with status {status}");
            }
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
        Commands::Setup { force } => {
            claudebox_core::setup::run_setup(force)?;
        }
    }

    Ok(())
}

/// Commit the `.rvf` file to git if the directory is inside a git repo.
/// Prints a warning if no git repo is found — without git, file operations
/// inside the VM are not recoverable.
fn git_commit_rvf(rvf_path: &std::path::Path, dir: &std::path::Path) {
    use std::process::Command;

    // Check whether we're inside a git repo.
    let in_repo = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !in_repo {
        eprintln!(
            "\nWarning: no git repository found in {}.\n\
             Without git, file changes made by Claude inside the VM cannot be undone.\n\
             Run `git init && git add . && git commit -m 'initial'` before starting.",
            dir.display()
        );
        return;
    }

    // Stage the .rvf file.
    let staged = Command::new("git")
        .args(["add", &rvf_path.to_string_lossy()])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !staged {
        eprintln!("Warning: could not stage {} in git.", rvf_path.display());
        return;
    }

    // Commit — non-fatal if it fails (e.g. nothing changed, no identity configured).
    let committed = Command::new("git")
        .args([
            "commit",
            "-m",
            &format!(
                "chore: add claudebox environment ({})",
                rvf_path.file_name().unwrap_or_default().to_string_lossy()
            ),
        ])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if committed {
        eprintln!(
            "Committed {} to git — file changes inside the VM are recoverable via git.",
            rvf_path.file_name().unwrap_or_default().to_string_lossy()
        );
    }
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
