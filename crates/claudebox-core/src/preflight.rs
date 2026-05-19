use anyhow::Result;

pub fn check_dependencies() -> Result<()> {
    // QEMU is the only hard requirement for the start command.
    // Firecracker/virtiofsd are the future Linux-native path; not required yet.
    check_single_dep("qemu-system-x86_64", "7.0.0")
        .map_err(|_| anyhow::anyhow!(
            "qemu-system-x86_64 not found.\n  macOS: brew install qemu\n  Ubuntu: apt install qemu-system-x86"
        ))?;
    check_single_dep("ssh", "0.0.0")
        .map_err(|_| anyhow::anyhow!(
            "ssh not found. Install OpenSSH: apt install openssh-client  (or equivalent)"
        ))?;
    // On Linux only: check /dev/kvm
    #[cfg(target_os = "linux")]
    check_kvm()?;
    Ok(())
}

pub fn check_single_dep(name: &str, _min_version: &str) -> Result<()> {
    use std::process::Command;
    Command::new(name)
        .arg("--version")
        .output()
        .map_err(|_| {
            anyhow::anyhow!(
                "Dependency '{}' not found. Install it with: <platform-specific instructions>",
                name
            )
        })?;
    // version enforcement is future work; _min_version intentionally unused
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_kvm() -> Result<()> {
    if !std::path::Path::new("/dev/kvm").exists() {
        anyhow::bail!("/dev/kvm not found. Enable KVM: modprobe kvm_intel (or kvm_amd)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_dep_returns_error() {
        // On CI / test machines without firecracker installed, this should return an error
        // not panic. Use a fake binary name to guarantee failure.
        let result = check_single_dep("definitely-not-a-real-binary-xyz123", "0.0.0");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("definitely-not-a-real-binary-xyz123"));
        assert!(msg.contains("not found"));
    }
}
