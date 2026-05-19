use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(target_os = "macos")]
const DEV_IMAGE_VERSION: &str = "0.1.0";

// Architecture-specific asset names and URLs.
// Compiled-in constants select the right assets for the host architecture.
#[cfg(target_arch = "aarch64")]
const DEV_IMAGE_NAME: &str = "claudebox-dev-0.1.0-aarch64.qcow2";
#[cfg(target_arch = "aarch64")]
const KERNEL_URL: &str =
    "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/kernel-aarch64";
#[cfg(target_arch = "aarch64")]
const INITRAMFS_URL: &str =
    "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/initramfs-aarch64";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const DEV_IMAGE_URL: &str =
    "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/claudebox-dev-0.1.0-aarch64.qcow2";

#[cfg(not(target_arch = "aarch64"))]
const DEV_IMAGE_NAME: &str = "claudebox-dev-0.1.0-x86_64.qcow2";
#[cfg(not(target_arch = "aarch64"))]
const KERNEL_URL: &str =
    "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/kernel-x86_64";
#[cfg(not(target_arch = "aarch64"))]
const INITRAMFS_URL: &str =
    "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/initramfs-x86_64";
#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
const DEV_IMAGE_URL: &str =
    "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/claudebox-dev-0.1.0-x86_64.qcow2";

/// Root of the claudebox data directory (`~/.claudebox`).
pub fn data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".claudebox"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/claudebox"))
}

/// Host architecture string used for asset sub-directories.
pub fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

pub fn default_kernel_path() -> PathBuf {
    data_dir().join("kernels").join(host_arch()).join("kernel")
}

pub fn default_initramfs_path() -> PathBuf {
    data_dir().join("kernels").join(host_arch()).join("initramfs")
}

pub fn default_rootfs_path() -> PathBuf {
    data_dir().join("rootfs").join(DEV_IMAGE_NAME)
}

/// Per-instance qcow2 overlay path for a given project.
/// Each running VM gets its own overlay so the base image stays read-only.
pub fn instance_overlay_path(project_id: &str) -> PathBuf {
    data_dir().join("vms").join(project_id).join("disk.qcow2")
}

/// PID file path for a running VM instance.
pub fn instance_pid_path(project_id: &str) -> PathBuf {
    data_dir().join("vms").join(project_id).join("qemu.pid")
}

/// Unix socket path used by the host-side vsock log bridge for a VM instance.
pub fn instance_vsock_path(project_id: &str) -> PathBuf {
    data_dir().join("vms").join(project_id).join("logs.sock")
}

/// Per-instance VM data directory (overlay, PID file, etc.).
pub fn instance_vm_dir(project_id: &str) -> PathBuf {
    data_dir().join("vms").join(project_id)
}

pub fn run_setup(force: bool) -> Result<()> {
    let data = data_dir();
    let kernels_dir = data.join("kernels").join(host_arch());
    let rootfs_dir = data.join("rootfs");

    std::fs::create_dir_all(&kernels_dir)
        .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", kernels_dir.display()))?;
    std::fs::create_dir_all(&rootfs_dir)
        .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", rootfs_dir.display()))?;

    eprintln!("claudebox setup ({})", host_arch());
    eprintln!("===============");

    setup_qemu(force)?;
    setup_kernel(&kernels_dir, force)?;

    #[cfg(target_os = "macos")]
    setup_rootfs_macos(&rootfs_dir, force)?;

    eprintln!("\nSetup complete. Get started:");
    eprintln!("  claudebox init myapp --lang node@22");
    eprintln!("  claudebox start myapp.rvf");

    Ok(())
}

fn setup_qemu(force: bool) -> Result<()> {
    // Check for the binary that matches the host architecture.
    let qemu_binary = match host_arch() {
        "aarch64" => "qemu-system-aarch64",
        _         => "qemu-system-x86_64",
    };

    let version_out = Command::new(qemu_binary).arg("--version").output();

    let already_installed = version_out
        .as_ref()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if already_installed && !force {
        let ver = version_out.unwrap().stdout;
        let ver_str = String::from_utf8_lossy(&ver);
        let first_line = ver_str.lines().next().unwrap_or("installed");
        eprintln!("[✓] QEMU: {first_line}");
        return Ok(());
    }

    install_qemu()
}

#[cfg(target_os = "macos")]
fn install_qemu() -> Result<()> {
    eprintln!("[→] QEMU not found — installing via Homebrew...");
    if !Command::new("brew")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        anyhow::bail!(
            "[✗] Homebrew not found. Install it first:\n    /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"\n    Then re-run: claudebox setup"
        );
    }
    let status = Command::new("brew")
        .args(["install", "qemu"])
        .status()
        .map_err(|e| anyhow::anyhow!("brew install qemu failed: {e}"))?;
    if !status.success() {
        anyhow::bail!("[✗] brew install qemu failed — run manually: brew install qemu");
    }
    eprintln!("[✓] QEMU installed via Homebrew");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn install_qemu() -> Result<()> {
    anyhow::bail!(
        "[✗] qemu-system not found.\n    Install: sudo apt install qemu-system  (Debian/Ubuntu)\n             sudo dnf install qemu-system   (Fedora/RHEL)"
    )
}

fn setup_kernel(kernels_dir: &Path, force: bool) -> Result<()> {
    let kernel_dest = kernels_dir.join("kernel");
    let initramfs_dest = kernels_dir.join("initramfs");

    if kernel_dest.exists() && initramfs_dest.exists() && !force {
        eprintln!("[✓] Kernel: {}", kernel_dest.display());
        eprintln!("[✓] Initramfs: {}", initramfs_dest.display());
        return Ok(());
    }

    // 1. Check local repo clone (kernels/<arch>/ alongside the binary or in cwd).
    let arch = host_arch();
    let local_candidates: Vec<PathBuf> = {
        let mut v = vec![PathBuf::from(format!("kernels/{arch}/kernel"))];
        if let Ok(exe) = std::env::current_exe() {
            for ancestor in exe.ancestors().skip(1).take(5) {
                v.push(ancestor.join(format!("kernels/{arch}/kernel")));
            }
        }
        v
    };

    for candidate in &local_candidates {
        if candidate.exists() {
            std::fs::copy(candidate, &kernel_dest)
                .map_err(|e| anyhow::anyhow!("failed to copy kernel: {e}"))?;
            eprintln!("[✓] Kernel copied to {}", kernel_dest.display());

            let initramfs_candidate = candidate.with_file_name("initramfs");
            if initramfs_candidate.exists() {
                std::fs::copy(&initramfs_candidate, &initramfs_dest)
                    .map_err(|e| anyhow::anyhow!("failed to copy initramfs: {e}"))?;
                eprintln!("[✓] Initramfs copied to {}", initramfs_dest.display());
            }
            return Ok(());
        }
    }

    // 2. Download kernel from GitHub releases.
    eprintln!("[→] Downloading kernel ({arch})...");
    eprintln!("    {KERNEL_URL}");
    download_file(KERNEL_URL, &kernel_dest)
        .map_err(|e| anyhow::anyhow!("[✗] Kernel download failed: {e}"))?;
    eprintln!("[✓] Kernel: {}", kernel_dest.display());

    // 3. Download initramfs from GitHub releases.
    eprintln!("[→] Downloading initramfs ({arch})...");
    eprintln!("    {INITRAMFS_URL}");
    download_file(INITRAMFS_URL, &initramfs_dest)
        .map_err(|e| anyhow::anyhow!("[✗] Initramfs download failed: {e}"))?;
    eprintln!("[✓] Initramfs: {}", initramfs_dest.display());

    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_rootfs_macos(rootfs_dir: &Path, force: bool) -> Result<()> {
    let dest = rootfs_dir.join(DEV_IMAGE_NAME);
    if dest.exists() && !force {
        eprintln!("[✓] Dev image: {}", dest.display());
        return Ok(());
    }

    let arch = host_arch();
    eprintln!("[→] Downloading claudebox-dev v{DEV_IMAGE_VERSION} ({arch}, ~300 MB)...");
    eprintln!("    {DEV_IMAGE_URL}");

    download_file(DEV_IMAGE_URL, &dest).map_err(|_| {
        anyhow::anyhow!(
            "[✗] Download failed.\n    \
             If the release isn't published yet, build locally:\n    \
             ./scripts/build-dev-image.sh --platform linux/{arch}\n    \
             cp images/output/{DEV_IMAGE_NAME} ~/.claudebox/rootfs/"
        )
    })?;

    eprintln!("[✓] Dev image: {}", dest.display());
    Ok(())
}

fn download_file(url: &str, dest: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| anyhow::anyhow!("curl not found: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(dest);
        anyhow::bail!("download failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_contains_claudebox() {
        assert!(data_dir().to_str().unwrap().contains("claudebox"));
    }

    #[test]
    fn default_paths_are_arch_namespaced() {
        let arch = host_arch();
        assert!(default_kernel_path().to_str().unwrap().contains(arch));
        assert!(default_initramfs_path().to_str().unwrap().contains(arch));
    }

    #[test]
    fn default_rootfs_is_dev_image() {
        assert!(default_rootfs_path().to_str().unwrap().contains("claudebox-dev"));
    }

    #[test]
    fn instance_paths_contain_project_id() {
        let id = "test-project-123";
        assert!(instance_overlay_path(id).to_str().unwrap().contains(id));
        assert!(instance_pid_path(id).to_str().unwrap().contains(id));
    }
}
