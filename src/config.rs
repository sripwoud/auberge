use crate::user_config::UserConfig;
use eyre::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub dns: DnsConfig,
    #[serde(default)]
    pub cloudflare: CloudflareConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DnsConfig {
    #[serde(default)]
    pub domain: String,
    #[serde(default = "default_ttl")]
    pub default_ttl: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CloudflareConfig {
    pub zone_id: Option<String>,
}

fn default_ttl() -> u32 {
    300
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = UserConfig::path()?;
        let contents = std::fs::read_to_string(&config_path).wrap_err_with(|| {
            format!(
                "Config not found at {}. Run `auberge config init` first.",
                config_path.display()
            )
        })?;
        toml::from_str(&contents).wrap_err("Failed to parse config.toml")
    }

    pub fn config_dir() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("auberge"))
            .ok_or_else(|| eyre::eyre!("Could not determine XDG config directory"))
    }

    pub fn data_dir() -> Result<PathBuf> {
        dirs::data_dir()
            .map(|p| p.join("auberge"))
            .ok_or_else(|| eyre::eyre!("Could not determine XDG data directory"))
    }
}

impl DnsConfig {
    pub fn zone_name(&self) -> &str {
        &self.domain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ttl() {
        let toml_str = r#"
            domain = "example.com"
        "#;
        let config: DnsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.default_ttl, 300);
    }
}
