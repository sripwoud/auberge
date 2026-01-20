use eyre::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub user: UserConfig,
    pub dns: DnsConfig,
    pub namecheap: NamecheapConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserConfig {
    pub username: String,
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsConfig {
    pub domain: String,
    pub subdomains: Vec<String>,
    #[serde(default = "default_ttl")]
    pub default_ttl: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NamecheapConfig {
    pub api_user: String,
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
    pub fn sld(&self) -> &str {
        self.domain.split('.').next().unwrap_or(&self.domain)
    }

    pub fn tld(&self) -> &str {
        self.domain
            .split('.')
            .nth(1)
            .unwrap_or_else(|| self.domain.split('.').last().unwrap_or(&self.domain))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_config_parsing() {
        let config = DnsConfig {
            domain: "example.com".to_string(),
            subdomains: vec!["www".to_string(), "api".to_string()],
            default_ttl: 300,
        };
        assert_eq!(config.sld(), "example");
        assert_eq!(config.tld(), "com");
    }

    #[test]
    fn test_default_ttl() {
        let toml_str = r#"
            domain = "example.com"
            subdomains = []
        "#;
        let config: DnsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.default_ttl, 300);
    }
}
