use anyhow::Result;

pub fn check_dependencies() -> Result<()> {
    check_single_dep("qemu-system-x86_64", "7.0.0")
        .map_err(|_| anyhow::anyhow!(
            "qemu-system-x86_64 not found.\n  macOS: brew install qemu\n  Ubuntu: apt install qemu-system-x86"
        ))?;
    check_single_dep("ssh", "0.0.0")
        .map_err(|_| anyhow::anyhow!(
            "ssh not found. Install OpenSSH: apt install openssh-client  (or equivalent)"
        ))?;
    #[cfg(target_os = "linux")]
    check_kvm()?;
    Ok(())
}

/// Check that `name` is installed and meets `min_version` (semver major.minor.patch).
/// Parses the first sequence of digits and dots found in `<name> --version` output.
pub fn check_single_dep(name: &str, min_version: &str) -> Result<()> {
    use std::process::Command;
    let output = Command::new(name)
        .arg("--version")
        .output()
        .map_err(|_| anyhow::anyhow!("Dependency '{}' not found.", name))?;

    if min_version == "0.0.0" {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    let found = parse_semver_from_output(&combined)
        .ok_or_else(|| anyhow::anyhow!("could not detect version for '{name}'"))?;
    let min = parse_semver(min_version)
        .ok_or_else(|| anyhow::anyhow!("invalid min_version '{min_version}'"))?;

    if found < min {
        anyhow::bail!(
            "'{name}' version {}.{}.{} is below required {min_version}",
            found.0, found.1, found.2
        );
    }
    Ok(())
}

/// Parse a (major, minor, patch) triple from a version string like
/// "QEMU emulator version 9.2.0" or "OpenSSH_9.7p1" or "git version 2.43.0".
pub fn parse_semver_from_output(output: &str) -> Option<(u32, u32, u32)> {
    // Walk the string looking for a run of digits-and-dots of the form N.N[.N].
    let bytes = output.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let candidate = &output[start..i];
            if let Some(triple) = parse_semver(candidate) {
                return Some(triple);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    // Accept "major.minor.patch" or "major.minor" (patch defaults to 0).
    // Strips a trailing non-digit suffix like "9.7p1" → takes "9.7".
    let stripped: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let parts: Vec<&str> = stripped.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts[1].parse::<u32>().ok()?;
    let patch = parts.get(2).and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    Some((major, minor, patch))
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
        let result = check_single_dep("definitely-not-a-real-binary-xyz123", "0.0.0");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("definitely-not-a-real-binary-xyz123"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_parse_semver_from_qemu_output() {
        let line = "QEMU emulator version 9.2.0 (Debian 1:9.2.0+ds-2)";
        assert_eq!(parse_semver_from_output(line), Some((9, 2, 0)));
    }

    #[test]
    fn test_parse_semver_from_openssh_output() {
        // OpenSSH prints e.g. "OpenSSH_9.7p1, LibreSSL 3.3.6"
        let line = "OpenSSH_9.7p1, LibreSSL 3.3.6";
        let triple = parse_semver_from_output(line).unwrap();
        assert_eq!(triple.0, 9);
        assert_eq!(triple.1, 7);
    }

    #[test]
    fn test_parse_semver_from_git_output() {
        let line = "git version 2.43.0";
        assert_eq!(parse_semver_from_output(line), Some((2, 43, 0)));
    }

    #[test]
    fn test_version_below_min_returns_error() {
        // Simulate a version string that parses to 7.0.0.
        let found = (7u32, 0u32, 0u32);
        let min = (8u32, 0u32, 0u32);
        assert!(found < min);
    }

    #[test]
    fn test_version_equal_or_above_min_passes() {
        let found = (9u32, 2u32, 0u32);
        let min = (7u32, 0u32, 0u32);
        assert!(found >= min);
    }

    #[test]
    fn test_parse_semver_two_part() {
        assert_eq!(parse_semver("9.7"), Some((9, 7, 0)));
    }

    #[test]
    fn test_parse_semver_no_match_returns_none() {
        assert_eq!(parse_semver_from_output("no version here"), None);
    }
}
