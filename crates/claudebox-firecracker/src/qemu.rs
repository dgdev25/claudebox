//! QEMU command-line builder for ClaudeBox microVM launches.
//!
//! Adapted from the ruvector reference implementation (MIT licensed).

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

/// Locate qemu-system-x86_64 on PATH or common install locations.
pub fn find_qemu(arch_str: &str) -> anyhow::Result<PathBuf> {
    let binary = match arch_str {
        "aarch64" => "qemu-system-aarch64",
        _ => "qemu-system-x86_64",
    };

    let candidates = [
        binary,
        &format!("/usr/bin/{binary}"),
        &format!("/usr/local/bin/{binary}"),
        &format!("/opt/homebrew/bin/{binary}"),
    ];

    for candidate in &candidates {
        if let Ok(output) = std::process::Command::new("which").arg(candidate).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok(PathBuf::from(path));
            }
        }
        let p = Path::new(candidate);
        if p.is_absolute() && p.exists() {
            return Ok(p.to_path_buf());
        }
    }

    anyhow::bail!(
        "{binary} not found. Install with: brew install qemu  (macOS) or apt install qemu-system-x86 (Linux)"
    )
}

/// Boot mode for a ClaudeBox microVM.
#[derive(Debug)]
enum BootMode<'a> {
    /// bzImage + initramfs cpio.gz (Linux-native path, no persistent disk)
    KernelInitramfs { initramfs: &'a Path },
    /// bzImage + persistent qcow2/img root disk (claudebox-dev image)
    KernelRootDisk { disk: &'a Path },
}

/// Build a QEMU invocation for a ClaudeBox microVM.
///
/// Boot mode is selected automatically:
/// - `initramfs_path` supplied → kernel + initramfs (Linux-native dev path)
/// - `rootfs_path` supplied (qcow2/img) → kernel + persistent root disk
///   (claudebox-dev image; works on macOS and Linux)
///
/// # Arguments
/// - `kernel_path` — extracted bzImage
/// - `initramfs_path` — cpio.gz initramfs (Linux only; None on macOS)
/// - `ssh_port` — host port forwarded to guest :22
/// - `memory_mb` — guest RAM in MiB
/// - `rootfs_path` — persistent dev image (qcow2/img); takes priority over initramfs
pub fn build_qemu_command(
    kernel_path: &Path,
    initramfs_path: Option<&Path>,
    ssh_port: u16,
    memory_mb: u32,
    rootfs_path: Option<&Path>,
) -> anyhow::Result<Command> {
    let qemu_bin = find_qemu("x86_64")?;
    let mut cmd = Command::new(&qemu_bin);

    let boot_mode = match (rootfs_path, initramfs_path) {
        (Some(disk), _) => BootMode::KernelRootDisk { disk },
        (None, Some(initramfs)) => BootMode::KernelInitramfs { initramfs },
        (None, None) => anyhow::bail!(
            "no rootfs or initramfs supplied — run `claudebox setup` first"
        ),
    };

    // Machine type: q35 for persistent disk (virtio-blk needs PCIe), microvm for initramfs
    #[cfg(target_os = "macos")]
    {
        match boot_mode {
            BootMode::KernelRootDisk { .. } => cmd.args(["-machine", "q35,accel=hvf:tcg"]),
            BootMode::KernelInitramfs { .. } => cmd.args(["-machine", "microvm,accel=hvf:tcg"]),
        };
        cmd.args(["-cpu", "host"]);
    }
    #[cfg(not(target_os = "macos"))]
    {
        match boot_mode {
            BootMode::KernelRootDisk { .. } => {
                if kvm_available() {
                    cmd.args(["-machine", "q35,accel=kvm"]);
                    cmd.args(["-cpu", "host"]);
                } else {
                    cmd.args(["-machine", "q35,accel=tcg"]);
                    cmd.args(["-cpu", "qemu64"]);
                }
            }
            BootMode::KernelInitramfs { .. } => {
                if kvm_available() {
                    cmd.args(["-machine", "microvm,accel=kvm"]);
                    cmd.args(["-cpu", "host"]);
                } else {
                    cmd.args(["-machine", "microvm,accel=tcg"]);
                    cmd.args(["-cpu", "qemu64"]);
                }
            }
        }
    }

    // Memory
    cmd.arg("-m").arg(format!("{memory_mb}M"));

    // Kernel is always used (QEMU loads it directly — no bootloader in the image)
    cmd.arg("-kernel").arg(kernel_path);

    match boot_mode {
        BootMode::KernelRootDisk { disk } => {
            let fmt = if disk.extension().is_some_and(|e| e == "qcow2") {
                "qcow2"
            } else {
                "raw"
            };
            cmd.arg("-drive").arg(format!(
                "file={},format={fmt},if=virtio",
                disk.display()
            ));
            cmd.arg("-append")
                .arg("root=/dev/vda rw console=ttyS0 panic=-1");
        }
        BootMode::KernelInitramfs { initramfs } => {
            cmd.arg("-initrd").arg(initramfs);
            cmd.arg("-append").arg("console=ttyS0 panic=-1");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kvm_detection_does_not_panic() {
        let _ = kvm_available();
    }

    #[test]
    fn find_qemu_returns_result() {
        // QEMU may not be installed in CI — just verify no panic and proper error type.
        let result = find_qemu("x86_64");
        match result {
            Ok(path) => assert!(path.to_str().unwrap().contains("qemu")),
            Err(e) => assert!(e.to_string().contains("qemu")),
        }
    }
}
