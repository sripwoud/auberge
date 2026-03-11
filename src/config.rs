use crate::user_config::UserConfig;
use eyre::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub domain: String,
    #[serde(default = "default_ttl")]
    pub default_ttl: u32,
    #[serde(default)]
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
        Self::parse(&contents)
    }

    fn parse(contents: &str) -> Result<Self> {
        let mut config: Self = toml::from_str(contents).wrap_err("Failed to parse config.toml")?;
        if config.domain.trim().is_empty() {
            eyre::bail!("'domain' is required in config.toml but is missing or empty");
        }
        config.domain = config.domain.trim().to_string();
        Ok(config)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ttl() {
        let config = Config::parse(r#"domain = "example.com""#).unwrap();
        assert_eq!(config.default_ttl, 300);
    }

    #[test]
    fn test_missing_domain_fails() {
        let err = Config::parse("default_ttl = 600").unwrap_err();
        assert!(err.to_string().contains("domain"));
    }

    #[test]
    fn test_empty_domain_fails() {
        let err = Config::parse(r#"domain = """#).unwrap_err();
        assert!(err.to_string().contains("domain"));
    }

    #[test]
    fn test_whitespace_domain_fails() {
        let err = Config::parse(r#"domain = "  ""#).unwrap_err();
        assert!(err.to_string().contains("domain"));
    }

    #[test]
    fn test_domain_is_trimmed() {
        let config = Config::parse(r#"domain = " example.com ""#).unwrap();
        assert_eq!(config.domain, "example.com");
    }
}
