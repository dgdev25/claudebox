use crate::manifest::NetworkPolicy;
use std::path::Path;

/// Add a domain to a `NetworkPolicy`, deduplicating and sorting.
pub fn add_domain_to_policy(policy: &mut NetworkPolicy, domain: &str) {
    let domain = domain.trim().to_string();
    if !domain.is_empty() && !policy.allow_domains.contains(&domain) {
        policy.allow_domains.push(domain);
        policy.allow_domains.sort();
        policy.allow_domains.dedup();
    }
}

/// Remove a domain from a `NetworkPolicy`.
pub fn remove_domain_from_policy(policy: &mut NetworkPolicy, domain: &str) {
    policy.allow_domains.retain(|d| d != domain);
}

/// Update the allowlist in an .rvf file.
///
/// Full implementation (reading MANIFEST_SEG, recompiling eBPF, writing
/// EBPF_SEG, updating WITNESS_SEG) is deferred to Phase 11 integration
/// when all sub-systems are available.
pub async fn run_update_allowlist(
    _rvf_path: &Path,
    _add: Vec<String>,
    _remove: Vec<String>,
) -> anyhow::Result<()> {
    anyhow::bail!(
        "run_update_allowlist requires rvf-runtime segment I/O — deferred to Phase 11 integration"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_domain_to_allowlist() {
        let mut policy = NetworkPolicy {
            allow_domains: vec!["registry.npmjs.org".into()],
            allow_localhost: true,
            dns_server: "1.1.1.1".into(),
        };
        add_domain_to_policy(&mut policy, "example.com");
        assert!(policy.allow_domains.contains(&"example.com".to_string()));
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
