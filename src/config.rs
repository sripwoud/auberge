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

    fn find_config_file() -> Result<PathBuf> {
        let current_dir = std::env::current_dir()?;

        let locations = vec![
            current_dir.join("config.toml"),
            current_dir.join("../config.toml"),
            dirs::home_dir()
                .map(|h| h.join(".config/auberge/config.toml"))
                .unwrap_or_else(|| PathBuf::from("~/.config/auberge/config.toml")),
        ];

        for path in locations {
            if path.exists() {
                return Ok(path);
            }
        }

        eyre::bail!(
            "Config file not found. Copy config.example.toml to config.toml and customize it.\n\
             Searched locations:\n  - ./config.toml\n  - ../config.toml\n  - ~/.config/auberge/config.toml"
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
