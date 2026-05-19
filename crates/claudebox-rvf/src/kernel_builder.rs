use std::path::PathBuf;

use claudebox_core::manifest::{Lang, SingleProfile};

pub struct KernelBuilder {
    pub profiles: Vec<SingleProfile>,
    pub project_id: String,
    pub ssh_public_key: String,
}

impl KernelBuilder {
    /// Returns the cached bzImage path if present; otherwise invokes
    /// `kernels/build.sh` to produce one.
    ///
    /// Docker is intentionally not used in the default ClaudeBox flow.
    /// Kernel provisioning must come from cache/prebuilt artifacts.
    pub async fn build(&self) -> anyhow::Result<PathBuf> {
        if let Some(cached) = self.cached_path() {
            return Ok(cached);
        }
        anyhow::bail!(
            "no cached kernel found at {}. \
             Docker-based kernel builds are disabled; provide a prebuilt kernel via \
             --kernel-from <path> or run `claudebox setup` to populate cache",
            self.cache_dir().join("bzImage").display()
        )
    }

    /// Returns a deterministic string key for this set of language profiles.
    ///
    /// Each profile is converted to `"lang-version"`, the resulting strings
    /// are sorted alphabetically, then joined with `"_"`.  Sort order is
    /// stable regardless of the order in which profiles were supplied.
    pub fn cache_key(&self) -> String {
        let mut keys: Vec<String> = self
            .profiles
            .iter()
            .map(|p| {
                let lang_str = match p.lang {
                    Lang::Node => "node",
                    Lang::Python => "python",
                    Lang::Rust => "rust",
                    Lang::Go => "go",
                };
                // Sanitize version: keep only alphanumeric, dot, and hyphen
                // characters so the resulting string is safe as a path component.
                let safe_version: String = p
                    .version
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
                    .collect();
                format!("{}-{}", lang_str, safe_version)
            })
            .collect();

        keys.sort();
        keys.join("_")
    }

    /// Returns the path to a cached bzImage if the file already exists on
    /// disk, or `None` if the cache directory is absent / the file is missing.
    pub fn cached_path(&self) -> Option<PathBuf> {
        let p = self.cache_dir().join("bzImage");
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }

    /// Returns the directory used to cache the compiled kernel for this
    /// profile set.  Uses `$HOME/.claudebox/kernels/<cache_key>/`.
    pub fn cache_dir(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".claudebox/kernels")
            .join(self.cache_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_is_sorted_deterministic() {
        let builder = KernelBuilder {
            profiles: vec![
                SingleProfile {
                    lang: Lang::Rust,
                    version: "1.87".into(),
                },
                SingleProfile {
                    lang: Lang::Node,
                    version: "22".into(),
                },
            ],
            project_id: "test".into(),
            ssh_public_key: "ssh-ed25519 AAAA...".into(),
        };
        assert_eq!(builder.cache_key(), "node-22_rust-1.87");
    }

    #[test]
    fn test_cache_key_single_lang() {
        let builder = KernelBuilder {
            profiles: vec![SingleProfile {
                lang: Lang::Node,
                version: "22".into(),
            }],
            project_id: "test".into(),
            ssh_public_key: "ssh-ed25519 AAAA...".into(),
        };
        assert_eq!(builder.cache_key(), "node-22");
    }

    #[test]
    fn test_cached_path_returns_none_when_absent() {
        let builder = KernelBuilder {
            profiles: vec![SingleProfile {
                lang: Lang::Go,
                version: "1.22".into(),
            }],
            project_id: "test".into(),
            ssh_public_key: "ssh-ed25519 AAAA...".into(),
        };
        // The cache dir for this profile won't exist in CI/test environments.
        assert!(builder.cached_path().is_none());
    }
}
