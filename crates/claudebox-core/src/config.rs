use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct DefaultsConfig {
    pub vcpus: u32,
    pub memory_mb: u64,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self { vcpus: 2, memory_mb: 4096 }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct WitnessConfig {
    pub max_entries: u64,
    pub retention_days: u32,
}

impl Default for WitnessConfig {
    fn default() -> Self {
        Self { max_entries: 10_000, retention_days: 30 }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct KernelConfig {
    pub staleness_warn_days: u32,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self { staleness_warn_days: 90 }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct EmbeddingConfig {
    pub chunk_tokens: usize,
    pub overlap_tokens: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self { chunk_tokens: 512, overlap_tokens: 64 }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct ClaudeBoxConfig {
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub witness: WitnessConfig,
    #[serde(default)]
    pub kernel: KernelConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
}

impl ClaudeBoxConfig {
    /// Load from `~/.claudebox/config.toml`, falling back to defaults if absent.
    pub fn load() -> anyhow::Result<Self> {
        let path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
            .join(".claudebox/config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read config: {e}"))?;
        toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))
    }

    /// Write to `~/.claudebox/config.toml`, creating directories if needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
            .join(".claudebox/config.toml");
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?;
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("failed to create config dir: {e}"))?;
        let content = toml::to_string(self)
            .map_err(|e| anyhow::anyhow!("failed to serialize config: {e}"))?;
        std::fs::write(&path, content)
            .map_err(|e| anyhow::anyhow!("failed to write config: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ClaudeBoxConfig::default();
        assert_eq!(config.defaults.vcpus, 2);
        assert_eq!(config.defaults.memory_mb, 4096);
        assert_eq!(config.witness.max_entries, 10_000);
        assert_eq!(config.witness.retention_days, 30);
        assert_eq!(config.kernel.staleness_warn_days, 90);
        assert_eq!(config.embedding.chunk_tokens, 512);
        assert_eq!(config.embedding.overlap_tokens, 64);
    }

    #[test]
    fn test_config_round_trip_toml() {
        let config = ClaudeBoxConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let restored: ClaudeBoxConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored.defaults.vcpus, config.defaults.vcpus);
        assert_eq!(restored.witness.max_entries, config.witness.max_entries);
    }
}
