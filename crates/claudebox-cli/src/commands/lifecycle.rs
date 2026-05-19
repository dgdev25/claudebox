use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::commands::adapters::{DefaultKernelResolver, DefaultVmAdapter, KernelResolver, VmAdapter};
use crate::commands::context::AppContext;
use crate::commands::output;

pub async fn handle_new_command(name: Option<String>) -> anyhow::Result<()> {
    println!("ClaudeBox Setup Wizard");
    println!("======================");

    let project_name = match name {
        Some(n) if !n.trim().is_empty() => n,
        _ => prompt_text("Project name", "myapp")?,
    };

    println!("\nEnvironment profile:");
    println!("  1) Light  (minimal profile, fast setup)");
    println!("  2) Full   (node, python, rust, go profile)");
    println!("  3) Custom (choose explicitly)");
    let profile = prompt_choice("Select profile [1-3]", &["1", "2", "3"], "1")?;

    let lang = match profile.as_str() {
        "1" => Vec::new(),
        "2" => vec![
            "node@latest".to_string(),
            "python@latest".to_string(),
            "rust@latest".to_string(),
            "go@latest".to_string(),
        ],
        _ => {
            let raw = prompt_text(
                "Custom languages (comma-separated, e.g. node@22,rust@latest)",
                "node@latest",
            )?;
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        }
    };

    let isolated = prompt_yes_no_with_default(
        "Use isolated mode (VM workspace mounted locally via sshfs)? (y/n)",
        true,
    );

    let start_now = prompt_yes_no_with_default("Start environment now after init? (y/n)", true);

    handle_init_command(project_name.clone(), lang, Vec::new(), None).await?;

    if start_now {
        let rvf = PathBuf::from(format!("{project_name}.rvf"));
        let mount_dir = if isolated {
            Some(std::env::current_dir()?.join(format!("{project_name}.isolated")))
        } else {
            None
        };
        handle_start_command(rvf, PathBuf::from("."), isolated, mount_dir, None).await?;
    } else {
        println!("\nNext step:");
        if isolated {
            println!("  claudebox start {}.rvf --isolated", project_name);
        } else {
            println!("  claudebox start {}.rvf", project_name);
        }
    }

    Ok(())
}

pub async fn handle_init_command(
    name: String,
    lang: Vec<String>,
    allow: Vec<String>,
    kernel_from: Option<PathBuf>,
) -> anyhow::Result<()> {
    let kernel_resolver = DefaultKernelResolver;
    let kernel_from = kernel_from.or_else(|| kernel_resolver.resolve_default_kernel());

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
    if output_path.exists() {
        print!(
            "{} already exists. Do you want me to delete and recreate it? (y/n): ",
            output_path.display()
        );
        let _ = io::stdout().flush();
        if prompt_yes_no() {
            std::fs::remove_file(&output_path)
                .map_err(|e| anyhow::anyhow!("failed to remove {}: {e}", output_path.display()))?;
        } else {
            anyhow::bail!("Initialization cancelled; existing RVF kept.");
        }
    }

    let builder = claudebox_rvf::builder::ApplianceBuilder::new(manifest)?;
    builder.build_skeleton(&output_path, kernel_from.as_deref())?;

    output::info(&format!("Created appliance: {}", output_path.display()));
    if kernel_from.is_none() {
        eprintln!(
            "Note: no kernel embedded. Supply --kernel-from <bzImage> \
             (or run `claudebox setup` for cached prebuilt kernel) to enable `claudebox start`."
        );
    }

    git_commit_rvf(&output_path, &output_dir);
    Ok(())
}

pub async fn handle_start_command(
    rvf: PathBuf,
    workspace: PathBuf,
    isolated: bool,
    mount_dir: Option<PathBuf>,
    rootfs: Option<PathBuf>,
) -> anyhow::Result<()> {
    claudebox_migrate::check_and_migrate(&rvf, true)?;

    let opts = claudebox_core::start::StartOptions {
        rvf: rvf.clone(),
        workspace: workspace.clone(),
    };
    let extracted = claudebox_core::start::extract_kernel(&opts)?;

    let manifest: claudebox_core::manifest::ClaudeBoxManifest =
        serde_json::from_str(&extracted.manifest_json)
            .map_err(|e| anyhow::anyhow!("corrupt manifest in rvf: {e}"))?;
    let guest_arch = manifest.kernel.arch.clone();

    let initramfs_path: Option<std::path::PathBuf> = {
        let setup_initramfs = claudebox_core::setup::default_initramfs_path();
        if setup_initramfs.exists() {
            Some(setup_initramfs)
        } else {
            let tmp_dir = extracted
                .kernel_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("kernel path has no parent"))?
                .to_path_buf();
            claudebox_firecracker::initramfs::build_initramfs(&tmp_dir).ok()
        }
    };

    let vm = DefaultVmAdapter;
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

    let disk_path = if let Some(base) = &base_rootfs {
        let overlay = claudebox_core::setup::instance_overlay_path(&manifest.project_id);
        vm.create_overlay(base, &overlay)?;
        Some(overlay)
    } else {
        None
    };

    let workspace_abs = workspace.canonicalize().unwrap_or_else(|_| workspace.clone());
    let workspace_for_vm = if isolated {
        None
    } else {
        Some(workspace_abs.as_path())
    };

    let mut qemu_cmd = vm.build_qemu_command(
        &extracted.kernel_path,
        initramfs_path.as_deref(),
        extracted.ssh_port,
        512,
        disk_path.as_deref(),
        workspace_for_vm,
        &guest_arch,
    )?;

    output::info(&format!(
        "Launching QEMU ({guest_arch}) for {} (ssh_port={})…",
        rvf.display(),
        extracted.ssh_port
    ));

    let child = crate::commands::adapters::spawn_vm(&mut qemu_cmd)?;

    let pid_path = claudebox_core::setup::instance_pid_path(&manifest.project_id);
    if let Some(parent) = pid_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&pid_path, child.id().to_string());

    output::info(&format!(
        "VM running (PID {}). SSH: ssh -p {} root@localhost",
        child.id(),
        extracted.ssh_port
    ));
    output::info("Password: claudebox");

    let mounted_path = if isolated {
        let default_mount = workspace_abs.join(format!("{}.isolated", manifest.project_name));
        let local_mount = mount_dir.unwrap_or(default_mount);
        match mount_isolated_workspace(extracted.ssh_port, &local_mount) {
            Ok(()) => {
                output::info(&format!(
                    "Isolated workspace mounted at {}",
                    local_mount.display()
                ));
                Some(local_mount)
            }
            Err(e) => {
                eprintln!("Warning: isolated mount failed: {e}");
                eprintln!("You can still connect directly: ssh -p {} root@localhost", extracted.ssh_port);
                None
            }
        }
    } else {
        None
    };

    let mut child = child;
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("QEMU wait failed: {e}"))?;

    if let Some(mount_point) = mounted_path.as_deref() {
        let _ = unmount_isolated_workspace(mount_point);
    }

    let _ = std::fs::remove_file(&pid_path);

    if !status.success() {
        anyhow::bail!("QEMU exited with status {status}");
    }
    Ok(())
}

fn mount_isolated_workspace(ssh_port: u16, mount_point: &Path) -> anyhow::Result<()> {
    if !command_exists("sshfs") {
        anyhow::bail!(
            "sshfs is not installed. Install it and retry isolated mode.\n\
             Ubuntu/Debian: sudo apt install sshfs\n\
             macOS: brew install macfuse sshfs-mac"
        );
    }

    std::fs::create_dir_all(mount_point).map_err(|e| {
        anyhow::anyhow!("failed to create mount directory {}: {e}", mount_point.display())
    })?;

    for _ in 0..20 {
        let mut cmd = Command::new("sshfs");
        cmd.args([
            "-o",
            "password_stdin",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "reconnect",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-p",
            &ssh_port.to_string(),
            "root@127.0.0.1:/workspace",
        ])
        .arg(mount_point)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to run sshfs: {e}"))?;
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(b"claudebox\n");
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    anyhow::bail!("unable to mount VM workspace via sshfs after retries")
}

fn unmount_isolated_workspace(mount_point: &Path) -> anyhow::Result<()> {
    let status = if command_exists("fusermount") {
        Command::new("fusermount")
            .args(["-u"])
            .arg(mount_point)
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run fusermount: {e}"))?
    } else {
        Command::new("umount")
            .arg(mount_point)
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run umount: {e}"))?
    };
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("unmount failed for {}", mount_point.display())
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub async fn handle_upgrade_kernel_command(
    rvf: &Path,
    kernel_from: Option<PathBuf>,
) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    let kernel_resolver = DefaultKernelResolver;
    let kernel_path = kernel_from
        .or_else(|| kernel_resolver.resolve_default_kernel())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no new kernel found; pass --kernel-from <path> or run `claudebox setup` first"
            )
        })?;
    let upgrader = claudebox_rvf::kernel_upgrade::KernelUpgrader {
        rvf_path: cx.paths.rvf.clone(),
    };
    let result = upgrader.upgrade(&kernel_path).await?;
    println!("Kernel upgraded:");
    println!("  from: {}", result.from_hash);
    println!("  to:   {}", result.to_hash);
    Ok(())
}

pub fn handle_kernel_command(
    rvf: &Path,
    action: crate::KernelAction,
) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    let manifest = &cx.manifest;
    match action {
        crate::KernelAction::Show => {
            println!("arch:         {}", manifest.kernel.arch);
            println!("ssh_port:     {}", manifest.kernel.ssh_port);
            println!("mcp_port:     {}", manifest.kernel.mcp_port);
            println!("kernel_built: {}", manifest.kernel_built_at);
        }
        crate::KernelAction::Cache => {
            let cache_dir = claudebox_core::setup::data_dir()
                .join("kernels")
                .join(&manifest.kernel.arch);
            std::fs::create_dir_all(&cache_dir)?;
            let opts = claudebox_core::start::StartOptions {
                rvf: cx.paths.rvf.clone(),
                workspace: std::path::PathBuf::from("."),
            };
            let extracted = claudebox_core::start::extract_kernel(&opts)?;
            let dest = cache_dir.join("kernel");
            std::fs::copy(&extracted.kernel_path, &dest)?;
            output::info(&format!("Kernel cached to {}", dest.display()));
        }
    }
    Ok(())
}

fn git_commit_rvf(rvf_path: &Path, dir: &Path) {
    use std::process::Command;

    let in_repo = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !in_repo {
        eprintln!("\nWarning: no git repository found in {}.", dir.display());
        eprintln!("Without git, file changes made by Claude inside the VM cannot be undone.");

        print!("Do you want me to initialize Git? (y/n): ");
        let _ = io::stdout().flush();

        let should_init = prompt_yes_no();

        if should_init {
            let init_ok = Command::new("git")
                .args(["init"])
                .current_dir(dir)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !init_ok {
                eprintln!("Warning: failed to run `git init`.");
                return;
            }

            let _ = Command::new("git")
                .args(["add", "."])
                .current_dir(dir)
                .status();

            let committed = Command::new("git")
                .args(["commit", "-m", "initial"])
                .current_dir(dir)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if committed {
                eprintln!("Initialized git repository and created initial commit.");
            } else {
                eprintln!("Initialized git repository. Initial commit was not created.");
            }
        } else {
            eprintln!("Skipping git initialization.");
        }
        return;
    }

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

fn prompt_yes_no() -> bool {
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map(|_| matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
        .unwrap_or(false)
}

fn prompt_yes_no_with_default(prompt: &str, default_yes: bool) -> bool {
    let suffix = if default_yes { " [Y/n]: " } else { " [y/N]: " };
    print!("{prompt}{suffix}");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return default_yes;
    }
    let trimmed = answer.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return default_yes;
    }
    matches!(trimmed.as_str(), "y" | "yes")
}

fn prompt_text(prompt: &str, default: &str) -> anyhow::Result<String> {
    print!("{prompt} [{default}]: ");
    let _ = io::stdout().flush();
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|e| anyhow::anyhow!("failed to read input: {e}"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_choice(prompt: &str, allowed: &[&str], default: &str) -> anyhow::Result<String> {
    loop {
        let value = prompt_text(prompt, default)?;
        if allowed.contains(&value.as_str()) {
            return Ok(value);
        }
        eprintln!("Invalid choice: {value}. Allowed: {}", allowed.join(", "));
    }
}
