//! QEMU command-line builder for ClaudeBox microVM launches.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Check if KVM is available on this host.
pub fn kvm_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/dev/kvm").exists()
            && std::fs::metadata("/dev/kvm")
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = m.permissions().mode();
                    mode & 0o666 != 0
                })
                .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    false
}

/// Locate the appropriate qemu-system binary for the given guest architecture.
pub fn find_qemu(guest_arch: &str) -> anyhow::Result<PathBuf> {
    let binary = match guest_arch {
        "aarch64" => "qemu-system-aarch64",
        "x86_64" => "qemu-system-x86_64",
        other => anyhow::bail!("unsupported guest architecture: {other}"),
    };

    let candidates = [
        binary.to_string(),
        format!("/usr/bin/{binary}"),
        format!("/usr/local/bin/{binary}"),
        format!("/opt/homebrew/bin/{binary}"),
    ];

    for candidate in &candidates {
        if let Ok(output) = std::process::Command::new("which").arg(candidate).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok(PathBuf::from(path));
            }
        }
        let p = Path::new(candidate.as_str());
        if p.is_absolute() && p.exists() {
            return Ok(p.to_path_buf());
        }
    }

    anyhow::bail!(
        "{binary} not found. Install with: brew install qemu  (macOS) or apt install qemu-system (Linux)"
    )
}

/// Select the host architecture — the arch of the running claudebox binary.
pub fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

/// Boot mode for a ClaudeBox microVM.
#[derive(Debug)]
enum BootMode<'a> {
    /// bzImage/Image + initramfs (no persistent disk — Linux-native path)
    KernelInitramfs { initramfs: &'a Path },
    /// bzImage/Image + persistent qcow2/img root disk.
    /// `initramfs` (when provided) is passed as `-initrd` so the kernel can
    /// load virtio-blk/ext4 modules before mounting the root device.
    KernelRootDisk {
        disk: &'a Path,
        initramfs: Option<&'a Path>,
    },
}

/// Build a QEMU invocation for a ClaudeBox microVM.
///
/// The guest architecture is selected to match the host so HVF/KVM acceleration
/// is available. On Apple Silicon (`aarch64`) we use `qemu-system-aarch64` with
/// `-machine virt,accel=hvf` for near-native speed. On Intel macOS we use
/// `qemu-system-x86_64` with `-machine q35,accel=hvf`. On Linux we prefer KVM.
///
/// # Arguments
/// - `kernel_path` — extracted kernel image (bzImage for x86_64, Image for aarch64)
/// - `initramfs_path` — cpio.gz initramfs; used as `-initrd` for both boot modes
/// - `ssh_port` — host port forwarded to guest :22
/// - `memory_mb` — guest RAM in MiB
/// - `rootfs_path` — persistent qcow2/img root disk; when `None` the initramfs IS the root
/// - `workspace_path` — host directory shared into guest at `/workspace` via virtio-9p
/// - `guest_arch` — target architecture (`"aarch64"` or `"x86_64"`)
pub fn build_qemu_command(
    kernel_path: &Path,
    initramfs_path: Option<&Path>,
    ssh_port: u16,
    memory_mb: u32,
    rootfs_path: Option<&Path>,
    workspace_path: Option<&Path>,
    guest_arch: &str,
) -> anyhow::Result<Command> {
    let qemu_bin = find_qemu(guest_arch)?;
    let mut cmd = Command::new(&qemu_bin);

    let boot_mode = match (rootfs_path, initramfs_path) {
        (Some(disk), initramfs) => BootMode::KernelRootDisk { disk, initramfs },
        (None, Some(initramfs)) => BootMode::KernelInitramfs { initramfs },
        (None, None) => anyhow::bail!(
            "no rootfs or initramfs supplied — run `claudebox setup` first"
        ),
    };

    // Machine type, accelerator, and CPU model.
    //
    // We match (OS, host_arch, guest_arch) at runtime:
    //   - Apple Silicon + aarch64 guest  → virt + HVF + host CPU  (fastest)
    //   - Intel macOS   + x86_64 guest   → q35  + HVF + host CPU
    //   - Linux         + x86_64 guest   → q35  + KVM + host CPU  (if /dev/kvm)
    //   - Linux         + x86_64 guest   → q35  + TCG + qemu64    (fallback)
    //   - Apple Silicon + x86_64 guest   → q35  + TCG + qemu64    (slow — avoid)
    let os = std::env::consts::OS;
    let harch = host_arch();

    let (machine, accel, cpu) = match (os, harch, guest_arch) {
        ("macos", "aarch64", "aarch64") => ("virt", "hvf", "host"),
        ("macos", "x86_64",  "x86_64")  => ("q35",  "hvf", "host"),
        ("linux", _,         "x86_64") if kvm_available() => ("q35", "kvm", "host"),
        ("linux", _,         "x86_64")  => ("q35",  "tcg", "qemu64"),
        ("linux", _,         "aarch64") => ("virt", "kvm", "host"),
        // Cross-arch emulation — slow TCG, best-effort
        (_,       _,         "aarch64") => ("virt", "tcg", "cortex-a57"),
        (_,       _,         "x86_64")  => ("q35",  "tcg", "qemu64"),
        _ => anyhow::bail!("unsupported host/guest combination: {harch}/{guest_arch} on {os}"),
    };

    cmd.args(["-machine", &format!("{machine},accel={accel}")]);
    cmd.args(["-cpu", cpu]);

    // Memory
    cmd.arg("-m").arg(format!("{memory_mb}M"));

    // Kernel — QEMU loads it directly, no bootloader needed
    cmd.arg("-kernel").arg(kernel_path);

    // Serial console device differs by architecture
    let console_dev = match guest_arch {
        "aarch64" => "ttyAMA0",
        _         => "ttyS0",
    };

    match boot_mode {
        BootMode::KernelRootDisk { disk, initramfs } => {
            let fmt = if disk.extension().is_some_and(|e| e == "qcow2") {
                "qcow2"
            } else {
                "raw"
            };
            cmd.arg("-drive").arg(format!(
                "file={},format={fmt},if=virtio",
                disk.display()
            ));

            // Initramfs loads kernel modules (virtio-blk, ext4) before mount.
            if let Some(rd) = initramfs {
                cmd.arg("-initrd").arg(rd);
            }

            cmd.arg("-append").arg(format!(
                "root=/dev/vda rw console={console_dev} panic=-1"
            ));

            // Workspace: host directory shared into guest via virtio-9p.
            if let Some(ws) = workspace_path {
                let ws_abs = ws.canonicalize().unwrap_or_else(|_| ws.to_path_buf());
                cmd.arg("-virtfs").arg(format!(
                    "local,path={},mount_tag=workspace,security_model=mapped-xattr,id=ws0",
                    ws_abs.display()
                ));
            }
        }
        BootMode::KernelInitramfs { initramfs } => {
            cmd.arg("-initrd").arg(initramfs);
            cmd.arg("-append").arg(format!("console={console_dev} panic=-1"));
        }
    }

    // Network: forward SSH port from host to guest :22
    cmd.arg("-netdev")
        .arg(format!("user,id=net0,hostfwd=tcp::{ssh_port}-:22"));
    cmd.args(["-device", "virtio-net-pci,netdev=net0"]);

    // No display, no reboot on panic
    cmd.arg("-nographic");
    cmd.arg("-no-reboot");

    Ok(cmd)
}

/// Create a per-instance qcow2 overlay on top of a read-only base image.
///
/// This prevents the base image from being write-locked and allows multiple
/// concurrent instances. The overlay records only the delta from the base.
pub fn create_instance_overlay(
    base_image: &Path,
    overlay_path: &Path,
) -> anyhow::Result<()> {
    if overlay_path.exists() {
        return Ok(()); // Resume existing instance
    }

    if let Some(parent) = overlay_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("cannot create overlay dir: {e}"))?;
    }

    let status = Command::new("qemu-img")
        .args(["create", "-f", "qcow2", "-b"])
        .arg(base_image)
        .arg("-F")
        .arg("qcow2")
        .arg(overlay_path)
        .status()
        .map_err(|e| anyhow::anyhow!("qemu-img not found: {e}"))?;

    anyhow::ensure!(status.success(), "qemu-img create overlay failed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kvm_detection_does_not_panic() {
        let _ = kvm_available();
    }

    #[test]
    fn find_qemu_x86_returns_result() {
        let result = find_qemu("x86_64");
        match result {
            Ok(path) => assert!(path.to_str().unwrap().contains("qemu")),
            Err(e) => assert!(e.to_string().contains("qemu")),
        }
    }

    #[test]
    fn find_qemu_aarch64_returns_result() {
        let result = find_qemu("aarch64");
        match result {
            Ok(path) => assert!(path.to_str().unwrap().contains("qemu")),
            Err(e) => assert!(e.to_string().contains("qemu")),
        }
    }

    #[test]
    fn find_qemu_unknown_arch_errors() {
        assert!(find_qemu("riscv128").is_err());
    }

    #[test]
    fn host_arch_is_known() {
        let arch = host_arch();
        assert!(arch == "x86_64" || arch == "aarch64", "unexpected arch: {arch}");
    }
}
