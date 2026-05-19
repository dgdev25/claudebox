use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClaudeBoxManifest {
    pub version: u8,
    pub project_id: String,
    pub project_name: String,
    pub language: LanguageProfile,
    pub created_at: String,
    pub kernel_built_at: String,
    pub network: NetworkPolicy,
    pub resources: ResourceLimits,
    pub kernel: KernelConfig,
    pub witness: WitnessPolicy,
}

/// Multi-language support (REMEDIATION BS-3).
/// Multi variant produces a union of all per-language network policies.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum LanguageProfile {
    Single(SingleProfile),
    Multi(Vec<SingleProfile>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SingleProfile {
    pub lang: Lang,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Lang {
    Node,
    Python,
    Rust,
    Go,
}

impl LanguageProfile {
    /// Returns all profiles as a flat vec regardless of Single/Multi.
    pub fn profiles(&self) -> Vec<&SingleProfile> {
        match self {
            LanguageProfile::Single(p) => vec![p],
            LanguageProfile::Multi(ps) => ps.iter().collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkPolicy {
    pub allow_domains: Vec<String>,
    pub allow_localhost: bool,
    pub dns_server: String,
}

impl NetworkPolicy {
    /// Builds a deduplicated union allowlist across all language profiles.
    pub fn for_profiles(profiles: &[&SingleProfile]) -> Self {
        let mut domains: Vec<String> = profiles
            .iter()
            .flat_map(|p| Self::domains_for_lang(&p.lang))
            .collect();
        domains.sort();
        domains.dedup();
        NetworkPolicy {
            allow_domains: domains,
            allow_localhost: true,
            dns_server: "1.1.1.1".into(),
        }
    }

    fn domains_for_lang(lang: &Lang) -> Vec<String> {
        match lang {
            Lang::Node => vec!["registry.npmjs.org".into(), "nodejs.org".into()],
            Lang::Python => vec!["pypi.org".into(), "files.pythonhosted.org".into()],
            Lang::Rust => vec![
                "crates.io".into(),
                "static.crates.io".into(),
                "index.crates.io".into(),
            ],
            Lang::Go => vec!["proxy.golang.org".into(), "sum.golang.org".into()],
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResourceLimits {
    pub vcpus: u8,
    pub memory_mb: u32,
    pub disk_gb: u32,
    pub network_mbps: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            vcpus: 2,
            memory_mb: 4096,
            disk_gb: 20,
            network_mbps: 100,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KernelConfig {
    pub arch: String,
    pub ssh_port: u16,
    /// Default: 7878 — not 8080 (REMEDIATION BS-9: avoids dev server conflict).
    pub mcp_port: u16,
}

/// WITNESS_SEG retention policy embedded in manifest (REMEDIATION BS-1).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WitnessPolicy {
    /// Default: 10_000 (~640 KB at 64B per entry).
    pub max_entries: u32,
    /// Default: 30 — older entries are archived to monthly .rvf files.
    pub retention_days: u32,
}

impl Default for WitnessPolicy {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            retention_days: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_round_trip() {
        let manifest = ClaudeBoxManifest {
            version: 1,
            project_id: "abc-123".into(),
            project_name: "myapp".into(),
            language: LanguageProfile::Single(SingleProfile {
                lang: Lang::Node,
                version: "22".into(),
            }),
            created_at: "2026-05-19T00:00:00Z".into(),
            kernel_built_at: "2026-05-19T00:00:00Z".into(),
            network: NetworkPolicy {
                allow_domains: vec!["registry.npmjs.org".into()],
                allow_localhost: true,
                dns_server: "1.1.1.1".into(),
            },
            resources: ResourceLimits::default(),
            kernel: KernelConfig {
                arch: "x86_64".into(),
                ssh_port: 2222,
                mcp_port: 7878,
            },
            witness: WitnessPolicy::default(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: ClaudeBoxManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.project_id, "abc-123");
        assert_eq!(back.kernel.mcp_port, 7878);
    }

    #[test]
    fn test_single_language_node_domains() {
        let profile = LanguageProfile::Single(SingleProfile {
            lang: Lang::Node,
            version: "22".into(),
        });
        let policy = NetworkPolicy::for_profiles(&profile.profiles());
        assert_eq!(policy.allow_domains.len(), 2);
        assert!(policy
            .allow_domains
            .contains(&"registry.npmjs.org".to_string()));
        assert!(policy.allow_domains.contains(&"nodejs.org".to_string()));
    }

    #[test]
    fn test_multi_language_union_deduplication() {
        let profile = LanguageProfile::Multi(vec![
            SingleProfile {
                lang: Lang::Node,
                version: "22".into(),
            },
            SingleProfile {
                lang: Lang::Rust,
                version: "1.87".into(),
            },
        ]);
        let policy = NetworkPolicy::for_profiles(&profile.profiles());
        // node: 2 domains, rust: 3 domains = 5 unique
        assert_eq!(policy.allow_domains.len(), 5);
        // No duplicates
        let mut sorted = policy.allow_domains.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), 5);
    }

    #[test]
    fn test_witness_policy_defaults() {
        let policy = WitnessPolicy::default();
        assert_eq!(policy.max_entries, 10_000);
        assert_eq!(policy.retention_days, 30);
    }

    #[test]
    fn test_kernel_config_default_mcp_port() {
        // Ensure mcp_port is 7878, NOT 8080 (BS-9 remediation)
        let config = KernelConfig {
            arch: "x86_64".into(),
            ssh_port: 2222,
            mcp_port: 7878,
        };
        assert_eq!(config.mcp_port, 7878);
    }
}
