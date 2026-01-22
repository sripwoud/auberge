use eyre::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub dns: DnsConfig,
    pub cloudflare: CloudflareConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsConfig {
    pub domain: String,
    #[serde(default = "default_ttl")]
    pub default_ttl: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudflareConfig {
    pub zone_id: Option<String>,
}

fn default_ttl() -> u32 {
    300
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::find_config_file()?;
        let contents = std::fs::read_to_string(&config_path)
            .wrap_err_with(|| format!("Failed to read config file: {}", config_path.display()))?;

        toml::from_str(&contents)
            .wrap_err("Failed to parse config.toml. Check the format against config.example.toml")
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

    fn find_config_file() -> Result<PathBuf> {
        let current_dir = std::env::current_dir()?;
        let xdg_config = dirs::config_dir()
            .ok_or_else(|| eyre::eyre!("Could not determine XDG config directory"))?
            .join("auberge/config.toml");

        let locations = vec![
            current_dir.join("config.toml"),
            current_dir.join("../config.toml"),
            xdg_config.clone(),
        ];

        for path in &locations {
            if path.exists() {
                return Ok(path.clone());
            }
        }

        let search_paths = locations
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");

        eyre::bail!(
            "Config file not found. Copy config.example.toml to one of:\n{}",
            search_paths
        )
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
