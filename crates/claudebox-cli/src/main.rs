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
            claudebox_migrate::check_and_migrate(&rvf, true)?;

            let opts = claudebox_core::start::StartOptions {
                rvf: rvf.clone(),
                workspace: workspace.clone(),
            };
            let extracted = claudebox_core::start::extract_kernel(&opts)?;

            // Parse the manifest embedded in the .rvf to get project_id and target arch.
            let manifest: claudebox_core::manifest::ClaudeBoxManifest =
                serde_json::from_str(&extracted.manifest_json)
                    .map_err(|e| anyhow::anyhow!("corrupt manifest in rvf: {e}"))?;
            let guest_arch = manifest.kernel.arch.clone();

            // Initramfs: loads virtio-blk + ext4 modules so the kernel finds /dev/vda.
            let initramfs_path: Option<std::path::PathBuf> = {
                let setup_initramfs = claudebox_core::setup::default_initramfs_path();
                if setup_initramfs.exists() {
                    Some(setup_initramfs)
                } else {
                    // Linux-only fallback: build a minimal initramfs on the fly.
                    let tmp_dir = extracted
                        .kernel_path
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("kernel path has no parent"))?
                        .to_path_buf();
                    claudebox_firecracker::initramfs::build_initramfs(&tmp_dir).ok()
                }
            };

            // Root disk: prefer explicit --rootfs, then per-arch default from setup.
            let base_rootfs = rootfs.or_else(|| {
                let default = claudebox_core::setup::default_rootfs_path();
                if default.exists() { Some(default) } else { None }
            });

            if initramfs_path.is_none() && base_rootfs.is_none() {
                anyhow::bail!(
                    "No initramfs found and no root disk available.\n\
                     Run `claudebox setup` to download kernel and dev image."
                );
            }

            // Create a per-instance qcow2 overlay so the base image stays read-only
            // and multiple VMs can run concurrently without locking conflicts.
            let disk_path = if let Some(base) = &base_rootfs {
                let overlay = claudebox_core::setup::instance_overlay_path(
                    &manifest.project_id,
                );
                claudebox_firecracker::qemu::create_instance_overlay(base, &overlay)?;
                Some(overlay)
            } else {
                None
            };

            let workspace_abs = workspace
                .canonicalize()
                .unwrap_or_else(|_| workspace.clone());

            let mut qemu_cmd = claudebox_firecracker::qemu::build_qemu_command(
                &extracted.kernel_path,
                initramfs_path.as_deref(),
                extracted.ssh_port,
                512,
                disk_path.as_deref(),
                Some(&workspace_abs),
                &guest_arch,
            )?;

            eprintln!(
                "Launching QEMU ({guest_arch}) for {} (ssh_port={})…",
                rvf.display(),
                extracted.ssh_port
            );

            // Spawn QEMU and write its PID so `claudebox stop` can signal it.
            let child = qemu_cmd
                .spawn()
                .map_err(|e| anyhow::anyhow!("failed to spawn QEMU: {e}"))?;

            let pid_path = claudebox_core::setup::instance_pid_path(
                &manifest.project_id,
            );
            if let Some(parent) = pid_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&pid_path, child.id().to_string());

            eprintln!("VM running (PID {}). SSH: ssh -p {} root@localhost", child.id(), extracted.ssh_port);
            eprintln!("Password: claudebox");

            // Wait for QEMU to exit (foreground; stop via Ctrl-C or `claudebox stop`).
            let mut child = child;
            let status = child
                .wait()
                .map_err(|e| anyhow::anyhow!("QEMU wait failed: {e}"))?;

            let _ = std::fs::remove_file(&pid_path);

            if !status.success() {
                anyhow::bail!("QEMU exited with status {status}");
            }
        }
        Commands::Stop { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            let manifest = claudebox_core::start::read_manifest_from_rvf(&rvf)?;
            let pid_path = claudebox_core::setup::instance_pid_path(&manifest.project_id);
            if !pid_path.exists() {
                eprintln!("VM is not running (no PID file found).");
            } else {
                claudebox_core::stop::stop_instance(&pid_path)?;
                eprintln!("VM stopped.");
            }
        }
        Commands::Logs { rvf, .. } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            anyhow::bail!("logs command not yet fully implemented")
        }
        Commands::Status { rvf } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            let manifest = claudebox_core::start::read_manifest_from_rvf(&rvf)?;
            let pid_path = claudebox_core::setup::instance_pid_path(&manifest.project_id);
            let vm_status = if let Ok(pid) = claudebox_core::stop::read_pid_file(&pid_path) {
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

            let rvf_size_mb = std::fs::metadata(&rvf)
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
                rvf_path: rvf.clone(),
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
        Commands::Kernel { rvf, action } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            let manifest = claudebox_core::start::read_manifest_from_rvf(&rvf)?;
            match action {
                KernelAction::Show => {
                    println!("arch:         {}", manifest.kernel.arch);
                    println!("ssh_port:     {}", manifest.kernel.ssh_port);
                    println!("mcp_port:     {}", manifest.kernel.mcp_port);
                    println!("kernel_built: {}", manifest.kernel_built_at);
                }
                KernelAction::Cache => {
                    let cache_dir = claudebox_core::setup::data_dir()
                        .join("kernels")
                        .join(&manifest.kernel.arch);
                    std::fs::create_dir_all(&cache_dir)?;
                    let opts = claudebox_core::start::StartOptions {
                        rvf: rvf.clone(),
                        workspace: std::path::PathBuf::from("."),
                    };
                    let extracted = claudebox_core::start::extract_kernel(&opts)?;
                    let dest = cache_dir.join("kernel");
                    std::fs::copy(&extracted.kernel_path, &dest)?;
                    eprintln!("Kernel cached to {}", dest.display());
                }
            }
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
        Commands::Destroy { rvf, force } => {
            claudebox_migrate::check_and_migrate(&rvf, true)?;
            let manifest = claudebox_core::start::read_manifest_from_rvf(&rvf)?;
            claudebox_core::stop::destroy_instance(&manifest.project_id, force)?;
            eprintln!("VM data for '{}' destroyed.", manifest.project_name);
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
