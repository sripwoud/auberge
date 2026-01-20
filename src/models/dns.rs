use serde::Deserialize;

fn default_ttl() -> u32 {
    300
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsConfig {
    pub domain: String,
    pub subdomains: Vec<String>,
    #[serde(default = "default_ttl")]
    pub default_ttl: u32,
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
    fn test_sld_tld_parsing() {
        let config = DnsConfig {
            domain: "sripwoud.xyz".to_string(),
            subdomains: vec![],
            default_ttl: 300,
        };
        assert_eq!(config.sld(), "sripwoud");
        assert_eq!(config.tld(), "xyz");
    }

    #[test]
    fn test_default_ttl() {
        let yaml = r#"
domain: example.com
subdomains: []
"#;
        let config: DnsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.default_ttl, 300);
    }
}
