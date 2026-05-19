use crate::manifest::{ClaudeBoxManifest, NetworkPolicy};
use crate::witness::{append_witness_entry, manifest_sidecar_path};
use claudebox_witness::WitnessEvent;
use std::path::Path;

/// Validate that `domain` is a well-formed RFC 1123 hostname.
///
/// Rejects empty strings, labels with non-`[a-zA-Z0-9-]` characters, labels
/// starting/ending with `-`, and any string containing whitespace or control
/// characters (which would allow Squid config injection via newlines).
pub fn validate_domain(domain: &str) -> anyhow::Result<()> {
    if domain.is_empty() {
        anyhow::bail!("domain name cannot be empty");
    }
    for label in domain.split('.') {
        if label.is_empty() {
            anyhow::bail!("invalid domain {:?}: empty label (leading/trailing dot or '..')", domain);
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            anyhow::bail!(
                "invalid domain {:?}: labels must contain only [a-zA-Z0-9-]",
                domain
            );
        }
        if label.starts_with('-') || label.ends_with('-') {
            anyhow::bail!(
                "invalid domain {:?}: labels cannot start or end with '-'",
                domain
            );
        }
    }
    Ok(())
}

/// Add a domain to a `NetworkPolicy`, deduplicating and sorting.
///
/// Returns an error if `domain` fails RFC 1123 hostname validation.
pub fn add_domain_to_policy(policy: &mut NetworkPolicy, domain: &str) -> anyhow::Result<()> {
    validate_domain(domain)?;
    let domain = domain.trim().to_string();
    if !domain.is_empty() && !policy.allow_domains.contains(&domain) {
        policy.allow_domains.push(domain);
        policy.allow_domains.sort();
        policy.allow_domains.dedup();
    }
    Ok(())
}

/// Remove a domain from a `NetworkPolicy`.
pub fn remove_domain_from_policy(policy: &mut NetworkPolicy, domain: &str) {
    policy.allow_domains.retain(|d| d != domain);
}

/// Update the allowlist in an `.rvf` appliance.
///
/// Reads the manifest from the `.rvf.manifest.json` sidecar (or falls back to
/// extracting it from the `.rvf` if no sidecar exists yet), applies add/remove
/// domain operations, writes the updated manifest back to the sidecar, and
/// appends an `AllowlistUpdate` witness entry.
pub async fn run_update_allowlist(
    rvf_path: &Path,
    add: Vec<String>,
    remove: Vec<String>,
) -> anyhow::Result<()> {
    if !rvf_path.exists() {
        anyhow::bail!("{} not found", rvf_path.display());
    }

    let mut manifest = read_live_manifest(rvf_path)?;

    let mut validated_add = Vec::new();
    for domain in &add {
        add_domain_to_policy(&mut manifest.network, domain)?;
        validated_add.push(domain.clone());
    }
    for domain in &remove {
        remove_domain_from_policy(&mut manifest.network, domain);
    }

    let sidecar = manifest_sidecar_path(rvf_path);
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| anyhow::anyhow!("manifest serialisation failed: {e}"))?;
    std::fs::write(&sidecar, json)
        .map_err(|e| anyhow::anyhow!("failed to write manifest sidecar: {e}"))?;

    append_witness_entry(
        rvf_path,
        WitnessEvent::AllowlistUpdate { added: validated_add, removed: remove },
    )?;

    Ok(())
}

/// Read the current live manifest for an `.rvf` appliance.
///
/// Prefers the `.rvf.manifest.json` sidecar (updated by allowlist/config commands);
/// falls back to extracting the genesis manifest from the `.rvf` file itself.
pub fn read_live_manifest(rvf_path: &Path) -> anyhow::Result<ClaudeBoxManifest> {
    let sidecar = manifest_sidecar_path(rvf_path);
    if sidecar.exists() {
        let json = std::fs::read_to_string(&sidecar)
            .map_err(|e| anyhow::anyhow!("failed to read manifest sidecar: {e}"))?;
        return serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("corrupt manifest sidecar: {e}"));
    }
    crate::start::read_manifest_from_rvf(rvf_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn build_test_rvf(dir: &std::path::Path) -> PathBuf {
        use rvf_runtime::{options::RvfOptions, RvfStore};
        let rvf = dir.join("test.rvf");
        let opts = RvfOptions { dimension: 1, ..Default::default() };
        let mut store = RvfStore::create(&rvf, opts).unwrap();
        store.embed_kernel(
            0x00, 0x01, 0, &[], 2222,
            Some(r#"{"version":1,"project_id":"test-id","project_name":"test","language":{"Single":{"lang":"Node","version":"22"}},"created_at":"","kernel_built_at":"","network":{"allow_domains":["registry.npmjs.org"],"allow_localhost":true,"dns_server":"1.1.1.1"},"resources":{"memory_mb":512,"vcpus":1,"disk_gb":8,"network_mbps":100},"kernel":{"arch":"x86_64","ssh_port":2222,"mcp_port":7878},"witness":{"max_entries":10000,"retention_days":30}}"#),
        ).unwrap();
        store.close().unwrap();
        rvf
    }

    #[tokio::test]
    async fn test_run_update_allowlist_adds_domain() {
        let dir = tempdir().unwrap();
        let rvf = build_test_rvf(dir.path());
        run_update_allowlist(&rvf, vec!["example.com".into()], vec![]).await.unwrap();
        let manifest = read_live_manifest(&rvf).unwrap();
        assert!(manifest.network.allow_domains.contains(&"example.com".to_string()));
        assert!(manifest.network.allow_domains.contains(&"registry.npmjs.org".to_string()));
    }

    #[tokio::test]
    async fn test_run_update_allowlist_removes_domain() {
        let dir = tempdir().unwrap();
        let rvf = build_test_rvf(dir.path());
        run_update_allowlist(&rvf, vec![], vec!["registry.npmjs.org".into()]).await.unwrap();
        let manifest = read_live_manifest(&rvf).unwrap();
        assert!(!manifest.network.allow_domains.contains(&"registry.npmjs.org".to_string()));
    }

    #[tokio::test]
    async fn test_run_update_allowlist_appends_witness_entry() {
        let dir = tempdir().unwrap();
        let rvf = build_test_rvf(dir.path());
        run_update_allowlist(&rvf, vec!["example.com".into()], vec![]).await.unwrap();
        let entries = crate::witness::load_witness_entries(&rvf).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].event, claudebox_witness::WitnessEvent::AllowlistUpdate { .. }));
    }

    #[tokio::test]
    async fn test_run_update_allowlist_rejects_invalid_domain() {
        let dir = tempdir().unwrap();
        let rvf = build_test_rvf(dir.path());
        let result = run_update_allowlist(&rvf, vec!["evil\ninjection".into()], vec![]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_update_allowlist_missing_rvf_returns_error() {
        let dir = tempdir().unwrap();
        let result = run_update_allowlist(&dir.path().join("missing.rvf"), vec![], vec![]).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_add_domain_to_allowlist() {
        let mut policy = NetworkPolicy {
            allow_domains: vec!["registry.npmjs.org".into()],
            allow_localhost: true,
            dns_server: "1.1.1.1".into(),
        };
        add_domain_to_policy(&mut policy, "example.com").unwrap();
        assert!(policy.allow_domains.contains(&"example.com".to_string()));
    }

    #[test]
    fn test_validate_domain_rejects_injection() {
        assert!(validate_domain("evil.com\nhttp_access allow all").is_err());
        assert!(validate_domain("").is_err());
        assert!(validate_domain(".leading-dot.com").is_err());
        assert!(validate_domain("-leading-hyphen.com").is_err());
        assert!(validate_domain("valid-domain.example.com").is_ok());
        assert!(validate_domain("registry.npmjs.org").is_ok());
    }

    #[test]
    fn test_remove_domain_from_allowlist() {
        let mut policy = NetworkPolicy {
            allow_domains: vec!["registry.npmjs.org".into(), "nodejs.org".into()],
            allow_localhost: true,
            dns_server: "1.1.1.1".into(),
        };
        remove_domain_from_policy(&mut policy, "nodejs.org");
        assert!(!policy.allow_domains.contains(&"nodejs.org".to_string()));
        assert_eq!(policy.allow_domains.len(), 1);
    }
}
