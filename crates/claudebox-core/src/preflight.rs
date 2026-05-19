use anyhow::Result;

pub fn check_dependencies() -> Result<()> {
    check_single_dep("firecracker", "1.7.0")?;
    check_single_dep("virtiofsd", "0.1.0")?;
    check_single_dep("clang", "15.0.0")?;
    check_single_dep("rvf", "0.1.0")?;
    check_single_dep("ssh", "0.0.0")?;
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
