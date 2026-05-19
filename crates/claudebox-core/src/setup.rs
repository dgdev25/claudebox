use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEV_IMAGE_VERSION: &str = "0.1.0";
const DEV_IMAGE_NAME: &str = "claudebox-dev-0.1.0.qcow2";
const DEV_IMAGE_URL: &str = "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/claudebox-dev-0.1.0.qcow2";
const KERNEL_URL: &str = "https://github.com/dgdev25/claudebox/releases/download/v0.1.0/bzImage-x86_64";

pub fn data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".claudebox"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/claudebox"))
}

pub fn default_kernel_path() -> PathBuf {
    data_dir().join("kernels").join("bzImage")
}

pub fn default_rootfs_path() -> PathBuf {
    data_dir().join("rootfs").join(DEV_IMAGE_NAME)
}

pub fn run_setup(force: bool) -> Result<()> {
    let data = data_dir();
    let kernels_dir = data.join("kernels");
    let rootfs_dir = data.join("rootfs");

    std::fs::create_dir_all(&kernels_dir)
        .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", kernels_dir.display()))?;
    std::fs::create_dir_all(&rootfs_dir)
        .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", rootfs_dir.display()))?;

    eprintln!("claudebox setup");
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
    let version_out = Command::new("qemu-system-x86_64")
        .arg("--version")
        .output();

    let already_installed = version_out
        .as_ref()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if already_installed && !force {
        let ver = version_out
            .unwrap()
            .stdout;
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
        "[✗] qemu-system-x86_64 not found.\n    Install: sudo apt install qemu-system-x86  (Debian/Ubuntu)\n             sudo dnf install qemu-system-x86   (Fedora/RHEL)"
    )
}

fn setup_kernel(kernels_dir: &Path, force: bool) -> Result<()> {
    let dest = kernels_dir.join("bzImage");
    if dest.exists() && !force {
        eprintln!("[✓] Kernel: {}", dest.display());
        return Ok(());
    }

    // 1. Check common local locations first (repo clone, adjacent to binary).
    let local_candidates: Vec<PathBuf> = {
        let mut v = vec![PathBuf::from("kernels/bzImage")];
        if let Ok(exe) = std::env::current_exe() {
            for ancestor in exe.ancestors().skip(1).take(5) {
                v.push(ancestor.join("kernels/bzImage"));
            }
        }
        v
    };

    for candidate in &local_candidates {
        if candidate.exists() {
            std::fs::copy(candidate, &dest)
                .map_err(|e| anyhow::anyhow!("failed to copy kernel: {e}"))?;
            eprintln!("[✓] Kernel copied to {}", dest.display());
            return Ok(());
        }
    }

    // 2. Download from GitHub releases.
    eprintln!("[→] Downloading kernel...");
    eprintln!("    {KERNEL_URL}");
    let status = Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(&dest)
        .arg(KERNEL_URL)
        .status()
        .map_err(|e| anyhow::anyhow!("curl not found: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&dest);
        anyhow::bail!("[✗] Kernel download failed — check network and retry.");
    }
    eprintln!("[✓] Kernel: {}", dest.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_rootfs_macos(rootfs_dir: &Path, force: bool) -> Result<()> {
    let dest = rootfs_dir.join(DEV_IMAGE_NAME);
    if dest.exists() && !force {
        eprintln!("[✓] Dev image: {}", dest.display());
        return Ok(());
    }

    eprintln!("[→] Downloading claudebox-dev v{DEV_IMAGE_VERSION} (~300 MB)...");
    eprintln!("    {DEV_IMAGE_URL}");

    let status = Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(&dest)
        .arg(DEV_IMAGE_URL)
        .status()
        .map_err(|e| anyhow::anyhow!("curl not found: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&dest);
        anyhow::bail!(
            "[✗] Download failed.\n    \
             If the release isn't published yet, build locally:\n    \
             ./scripts/build-dev-image.sh\n    \
             cp images/output/{DEV_IMAGE_NAME} ~/.claudebox/rootfs/"
        );
    }
    eprintln!("[✓] Dev image: {}", dest.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_under_home() {
        let d = data_dir();
        assert!(d.to_str().unwrap().contains("claudebox"));
    }

    #[test]
    fn default_paths_are_under_data_dir() {
        let base = data_dir();
        assert!(default_kernel_path().starts_with(&base));
        assert!(default_rootfs_path().starts_with(&base));
    }

    #[test]
    fn default_rootfs_is_dev_image() {
        assert!(default_rootfs_path()
            .to_str()
            .unwrap()
            .contains("claudebox-dev"));
    }
}
