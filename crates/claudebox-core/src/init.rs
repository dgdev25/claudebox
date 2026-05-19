use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::manifest::{
    ClaudeBoxManifest, KernelConfig, Lang, LanguageProfile, NetworkPolicy, ResourceLimits,
    SingleProfile, WitnessPolicy,
};

/// Options supplied by the user when running `claudebox init`.
pub struct InitOptions {
    pub name: String,
    /// Language specifiers such as `"node@22"` or `"rust@1.87"`.
    pub lang: Vec<String>,
    /// Additional domains to allow beyond the language-default set.
    pub allow: Vec<String>,
    /// Optional path to an existing kernel image to import rather than build.
    pub kernel_from: Option<PathBuf>,
}

/// Parse a language specifier string (`"node@22"`) into a [`SingleProfile`].
fn parse_lang_spec(spec: &str) -> anyhow::Result<SingleProfile> {
    let (lang_str, version) = if let Some((l, v)) = spec.split_once('@') {
        (l, v.to_string())
    } else {
        (spec, "latest".to_string())
    };

    let lang = match lang_str.trim().to_ascii_lowercase().as_str() {
        "node" | "nodejs" => Lang::Node,
        "python" | "py" => Lang::Python,
        "rust" | "rs" => Lang::Rust,
        "go" | "golang" => Lang::Go,
        other => anyhow::bail!("unknown language: {other:?}; supported: node, python, rust, go"),
    };

    Ok(SingleProfile { lang, version })
}

fn detect_lang_profiles(project_dir: &Path) -> Vec<SingleProfile> {
    let mut profiles = Vec::new();

    if project_dir.join("package.json").exists() {
        profiles.push(SingleProfile {
            lang: Lang::Node,
            version: "latest".to_string(),
        });
    }
    if project_dir.join("pyproject.toml").exists()
        || project_dir.join("requirements.txt").exists()
    {
        profiles.push(SingleProfile {
            lang: Lang::Python,
            version: "latest".to_string(),
        });
    }
    if project_dir.join("Cargo.toml").exists() {
        profiles.push(SingleProfile {
            lang: Lang::Rust,
            version: "latest".to_string(),
        });
    }
    if project_dir.join("go.mod").exists() {
        profiles.push(SingleProfile {
            lang: Lang::Go,
            version: "latest".to_string(),
        });
    }

    profiles
}

/// Build a [`ClaudeBoxManifest`] from `InitOptions` without performing any I/O.
///
/// Language strings are parsed into [`SingleProfile`]s, the network policy is
/// computed as the union of per-language defaults, and any extra domains from
/// `opts.allow` are merged in.
pub fn build_manifest_from_opts(opts: &InitOptions) -> anyhow::Result<ClaudeBoxManifest> {
    anyhow::ensure!(!opts.name.is_empty(), "project name must not be empty");

    let profiles: Vec<SingleProfile> = opts
        .lang
        .iter()
        .map(|s| parse_lang_spec(s).with_context(|| format!("parsing language spec {s:?}")))
        .collect::<anyhow::Result<_>>()?;

    // No --lang → base environment; the dev image has common tools pre-installed.
    let language = match profiles.len() {
        0 => LanguageProfile::Multi(vec![]),
        1 => LanguageProfile::Single(profiles.into_iter().next().unwrap()),
        _ => LanguageProfile::Multi(profiles),
    };

    let profile_refs: Vec<&SingleProfile> = language.profiles();
    let mut network = NetworkPolicy::for_profiles(&profile_refs);

    // Merge extra domains from `--allow`.
    for domain in &opts.allow {
        let d = domain.trim().to_string();
        if !d.is_empty() && !network.allow_domains.contains(&d) {
            network.allow_domains.push(d);
        }
    }
    network.allow_domains.sort();
    network.allow_domains.dedup();

    let now = chrono::Utc::now().to_rfc3339();

    Ok(ClaudeBoxManifest {
        version: 1,
        project_id: uuid::Uuid::new_v4().to_string(),
        project_name: opts.name.clone(),
        language,
        created_at: now.clone(),
        kernel_built_at: now,
        network,
        resources: ResourceLimits::default(),
        kernel: KernelConfig {
            arch: std::env::consts::ARCH.into(),
            ssh_port: 2222,
            mcp_port: 7878,
        },
        witness: WitnessPolicy::default(),
    })
}

/// Orchestrate the full `claudebox init` flow.
///
/// Stub: parses the manifest and returns it for the caller to use.
/// Appliance file writing is handled by the CLI layer (which has access to
/// both `claudebox-core` and `claudebox-rvf` without a circular dependency).
pub async fn run_init(opts: InitOptions, output_dir: &Path) -> anyhow::Result<ClaudeBoxManifest> {
    let mut opts = opts;
    if opts.lang.is_empty() {
        let detected = detect_lang_profiles(output_dir);
        if !detected.is_empty() {
            opts.lang = detected
                .into_iter()
                .map(|p| {
                    let name = match p.lang {
                        Lang::Node => "node",
                        Lang::Python => "python",
                        Lang::Rust => "rust",
                        Lang::Go => "go",
                    };
                    format!("{name}@{}", p.version)
                })
                .collect();
        }
    }
    build_manifest_from_opts(&opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_init_manifest_built_correctly_for_single_lang() {
        let opts = InitOptions {
            name: "myapp".into(),
            lang: vec!["node@22".into()],
            allow: vec![],
            kernel_from: None,
        };
        let manifest = build_manifest_from_opts(&opts).unwrap();
        assert_eq!(manifest.project_name, "myapp");
        assert_eq!(manifest.version, 1);
        match &manifest.language {
            LanguageProfile::Single(p) => assert!(matches!(p.lang, Lang::Node)),
            _ => panic!("Expected Single"),
        }
        assert!(manifest
            .network
            .allow_domains
            .contains(&"registry.npmjs.org".to_string()));
    }

    #[test]
    fn test_detect_lang_profiles_node_and_rust() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\nversion='0.1.0'\n").unwrap();

        let detected = detect_lang_profiles(dir.path());
        assert!(detected.iter().any(|p| matches!(p.lang, Lang::Node)));
        assert!(detected.iter().any(|p| matches!(p.lang, Lang::Rust)));
    }
}
