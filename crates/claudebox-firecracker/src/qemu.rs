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
/// Uses `-machine microvm,accel=tcg` for portability (works on macOS
/// without KVM). On Linux with KVM available, `accel=kvm` is preferred.
///
/// # Arguments
/// - `kernel_path` — path to the extracted bzImage
/// - `initramfs_path` — optional path to the initramfs cpio.gz
/// - `ssh_port` — host-side SSH port forwarded to guest :22
/// - `memory_mb` — guest RAM in MiB
/// - `workspace_path` — not used in the command itself yet; reserved for
///   virtiofs integration in a later phase
pub fn build_qemu_command(
    kernel_path: &Path,
    initramfs_path: Option<&Path>,
    ssh_port: u16,
    memory_mb: u32,
    _workspace_path: &Path,
) -> anyhow::Result<Command> {
    let qemu_bin = find_qemu("x86_64")?;
    let mut cmd = Command::new(&qemu_bin);

    // Machine type: prefer HVF on macOS, KVM on Linux, TCG as fallback.
    #[cfg(target_os = "macos")]
    {
        // Apple Silicon or Intel Mac — try hvf, fall back to tcg
        cmd.args(["-machine", "microvm,accel=hvf:tcg"]);
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

    // Kernel image
    cmd.arg("-kernel").arg(kernel_path);

    // Initramfs (if provided)
    if let Some(initrd) = initramfs_path {
        cmd.arg("-initrd").arg(initrd);
    }

    // Kernel command line
    cmd.arg("-append").arg("console=ttyS0 panic=-1");

    // Network: forward SSH port
    cmd.arg("-netdev").arg(format!(
        "user,id=net0,hostfwd=tcp::{ssh_port}-:22"
    ));
    cmd.args(["-device", "virtio-net-pci,netdev=net0"]);

    // No graphics, no reboot on panic
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
