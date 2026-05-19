use std::path::{Path, PathBuf};

use crate::commands::adapters::{DefaultKernelResolver, DefaultVmAdapter, KernelResolver, VmAdapter};
use crate::commands::context::AppContext;
use crate::commands::output;

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
    anyhow::ensure!(
        !output_path.exists(),
        "{} already exists — remove it before re-initialising",
        output_path.display()
    );

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

    let mut qemu_cmd = vm.build_qemu_command(
        &extracted.kernel_path,
        initramfs_path.as_deref(),
        extracted.ssh_port,
        512,
        disk_path.as_deref(),
        Some(&workspace_abs),
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

    let mut child = child;
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("QEMU wait failed: {e}"))?;

    let _ = std::fs::remove_file(&pid_path);

    if !status.success() {
        anyhow::bail!("QEMU exited with status {status}");
    }
    Ok(())
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
        eprintln!(
            "\nWarning: no git repository found in {}.\n\
             Without git, file changes made by Claude inside the VM cannot be undone.\n\
             Run `git init && git add . && git commit -m 'initial'` before starting.",
            dir.display()
        );
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
