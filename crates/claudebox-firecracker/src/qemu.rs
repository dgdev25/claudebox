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

/// Build a QEMU invocation for a ClaudeBox microVM.
///
/// Two boot modes:
/// - **Kernel + initramfs**: supply `kernel_path` + `initramfs_path` (Linux, or
///   macOS with a pre-built Linux initramfs placed by `claudebox setup`)
/// - **Disk image** (macOS primary path): supply `rootfs_path` to a `.qcow2`
///   or `.img`; the machine boots via BIOS from the disk, no kernel arg needed
///
/// # Arguments
/// - `kernel_path` — extracted bzImage (ignored in disk-image mode)
/// - `initramfs_path` — cpio.gz initramfs; `None` switches to disk-image mode
/// - `ssh_port` — host-side port forwarded to guest :22
/// - `memory_mb` — guest RAM in MiB
/// - `rootfs_path` — optional disk image (`.qcow2` / `.img`) for macOS path
pub fn build_qemu_command(
    kernel_path: &Path,
    initramfs_path: Option<&Path>,
    ssh_port: u16,
    memory_mb: u32,
    rootfs_path: Option<&Path>,
) -> anyhow::Result<Command> {
    let qemu_bin = find_qemu("x86_64")?;
    let mut cmd = Command::new(&qemu_bin);

    let disk_image_mode = initramfs_path.is_none() && rootfs_path.is_some();

    // Machine type
    #[cfg(target_os = "macos")]
    {
        // macOS: prefer HVF (hardware), fall back to TCG (software)
        // microvm machine type requires KVM; use q35 for macOS compatibility
        if disk_image_mode {
            cmd.args(["-machine", "q35,accel=hvf:tcg"]);
        } else {
            cmd.args(["-machine", "microvm,accel=hvf:tcg"]);
        }
        cmd.args(["-cpu", "host"]);
    }
    #[cfg(not(target_os = "macos"))]
    {
        if kvm_available() {
            cmd.args(["-machine", "microvm,accel=kvm"]);
            cmd.args(["-cpu", "host"]);
        } else {
            cmd.args(["-machine", "microvm,accel=tcg"]);
            cmd.args(["-cpu", "qemu64"]);
        }
    }

    // Memory
    cmd.arg("-m").arg(format!("{memory_mb}M"));

    if disk_image_mode {
        // Disk image boot (macOS path) — BIOS boots from the image, no -kernel needed
        let disk = rootfs_path.unwrap();
        let fmt = if disk.extension().is_some_and(|e| e == "qcow2") {
            "qcow2"
        } else {
            "raw"
        };
        cmd.arg("-drive").arg(format!(
            "file={},format={fmt},if=virtio",
            disk.display()
        ));
    } else {
        // Kernel + initramfs boot (Linux native or pre-built initramfs)
        cmd.arg("-kernel").arg(kernel_path);
        if let Some(initrd) = initramfs_path {
            cmd.arg("-initrd").arg(initrd);
        }
        cmd.arg("-append").arg("console=ttyS0 panic=-1");
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
