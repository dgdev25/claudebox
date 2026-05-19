use crate::manifest::NetworkPolicy;
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
