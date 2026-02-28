use eyre::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct UserConfig {
    path: PathBuf,
    table: toml::Table,
}

const TEMPLATE: &str = r#"[dns]
domain = ""

[cloudflare]
# zone_id = ""

[identity]
admin_user_name = ""
admin_user_email = ""
primary_domain = ""
ssh_port = 22022

[api_tokens]
cloudflare_dns_api_token = ""
tailscale_authkey = ""
namecheap_api_key = ""
namecheap_api_user = ""
namecheap_client_ip = ""

[baikal]
baikal_subdomain = ""
baikal_admin_password = ""

[colporteur]
colporteur_subdomain = ""
colporteur_feeds_password = ""

[blocky]
blocky_subdomain = ""

[booklore]
booklore_subdomain = ""
booklore_db_password = ""
booklore_admin_user = ""
booklore_admin_password = ""

[freshrss]
freshrss_subdomain = ""

[navidrome]
navidrome_subdomain = ""

[webdav]
webdav_subdomain = ""
webdav_password = ""

[yourls]
yourls_subdomain = ""
yourls_db_password = ""
yourls_admin_user = ""
yourls_admin_password = ""
yourls_cookiekey = ""
yourls_api_signature = ""

[openclaw]
openclaw_gateway_token = ""
openclaw_claude_ai_session_key = ""
openclaw_claude_web_session_key = ""
openclaw_claude_web_cookie = ""
"#;

const SENSITIVE_SECTIONS: &[&str] = &[
    "api_tokens",
    "baikal",
    "colporteur",
    "booklore",
    "webdav",
    "yourls",
    "openclaw",
];

impl UserConfig {
    pub fn path() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("auberge/config.toml"))
            .ok_or_else(|| eyre::eyre!("Could not determine XDG config directory"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            eyre::bail!(
                "Config file not found at {}. Run `auberge config init` to create it.",
                path.display()
            );
        }
        let contents = fs::read_to_string(&path)
            .wrap_err_with(|| format!("Failed to read {}", path.display()))?;
        let table: toml::Table =
            toml::from_str(&contents).wrap_err("Failed to parse config.toml")?;
        Ok(Self { path, table })
    }

    pub fn init() -> Result<PathBuf> {
        let path = Self::path()?;
        if path.exists() {
            eyre::bail!("Config already exists at {}", path.display());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .wrap_err_with(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(&path, TEMPLATE).wrap_err("Failed to write config template")?;
        Self::enforce_permissions(&path)?;
        Ok(path)
    }

    pub fn get(&self, key: &str) -> Option<String> {
        for (_section, value) in &self.table {
            if let toml::Value::Table(inner) = value
                && let Some(v) = inner.get(key)
            {
                return value_to_string(v);
            }
        }
        self.table.get(key).and_then(value_to_string)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<bool> {
        for (_section, section_value) in self.table.iter_mut() {
            if let toml::Value::Table(inner) = section_value
                && inner.contains_key(key)
            {
                inner.insert(key.to_string(), toml::Value::String(value.to_string()));
                self.save()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn remove(&mut self, key: &str) -> Result<bool> {
        for (_section, section_value) in self.table.iter_mut() {
            if let toml::Value::Table(inner) = section_value
                && inner.remove(key).is_some()
            {
                self.save()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn keys_redacted(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for (section, value) in &self.table {
            if let toml::Value::Table(inner) = value {
                let is_sensitive = SENSITIVE_SECTIONS.contains(&section.as_str());
                for (key, val) in inner {
                    let display = if is_sensitive {
                        match val {
                            toml::Value::String(s) if s.is_empty() => "(empty)".to_string(),
                            toml::Value::String(_) => "****".to_string(),
                            other => value_to_string(other).unwrap_or_default(),
                        }
                    } else {
                        value_to_string(val).unwrap_or_default()
                    };
                    result.push((key.clone(), display));
                }
            }
        }
        result
    }

    pub fn flatten_for_ansible(&self) -> BTreeMap<String, String> {
        flatten_toml(&self.table)
    }

    fn save(&self) -> Result<()> {
        let contents = toml::to_string_pretty(&self.table).wrap_err("Failed to serialize TOML")?;
        fs::write(&self.path, contents)
            .wrap_err_with(|| format!("Failed to write {}", self.path.display()))?;
        Self::enforce_permissions(&self.path)?;
        Ok(())
    }

    fn enforce_permissions(path: &PathBuf) -> Result<()> {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .wrap_err_with(|| format!("Failed to set permissions on {}", path.display()))
    }
}

fn value_to_string(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

fn flatten_toml(table: &toml::Table) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (key, value) in table {
        match value {
            toml::Value::Table(inner) => result.extend(flatten_toml(inner)),
            other => {
                if let Some(s) = value_to_string(other) {
                    result.insert(key.clone(), s);
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_flatten_toml() {
        let toml_str = r#"
            [dns]
            domain = "example.com"

            [identity]
            ssh_port = 22022
            admin_user_name = "alice"
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let flat = flatten_toml(&table);
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
        assert_eq!(flat.get("admin_user_name").unwrap(), "alice");
        assert!(!flat.contains_key("dns"));
        assert!(!flat.contains_key("identity"));
    }

    #[test]
    fn test_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = UserConfig {
            path: config_path.clone(),
            table: toml::from_str(TEMPLATE).unwrap(),
        };

        assert!(config.set("admin_user_name", "bob").unwrap());
        assert_eq!(config.get("admin_user_name").unwrap(), "bob");

        let reloaded_contents = fs::read_to_string(&config_path).unwrap();
        let reloaded_table: toml::Table = toml::from_str(&reloaded_contents).unwrap();
        let reloaded = UserConfig {
            path: config_path,
            table: reloaded_table,
        };
        assert_eq!(reloaded.get("admin_user_name").unwrap(), "bob");
    }

    #[test]
    fn test_permissions_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();

        UserConfig::enforce_permissions(&config_path).unwrap();
        let perms = fs::metadata(&config_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn test_template_parses() {
        let table: toml::Table = toml::from_str(TEMPLATE).unwrap();
        assert!(table.contains_key("dns"));
        assert!(table.contains_key("identity"));
        assert!(table.contains_key("api_tokens"));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let table: toml::Table = toml::from_str(TEMPLATE).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        assert!(config.get("nonexistent_key").is_none());
    }

    #[test]
    fn test_remove_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = UserConfig {
            path: config_path,
            table: toml::from_str(TEMPLATE).unwrap(),
        };
        config.set("admin_user_name", "test").unwrap();
        assert!(config.remove("admin_user_name").unwrap());
        assert!(config.get("admin_user_name").is_none());
    }

    #[test]
    fn test_keys_redacted() {
        let toml_str = r#"
            [identity]
            admin_user_name = "alice"

            [api_tokens]
            cloudflare_dns_api_token = "secret123"

            [baikal]
            baikal_admin_password = ""
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let items = config.keys_redacted();
        let map: BTreeMap<_, _> = items.into_iter().collect();
        assert_eq!(map.get("admin_user_name").unwrap(), "alice");
        assert_eq!(map.get("cloudflare_dns_api_token").unwrap(), "****");
        assert_eq!(map.get("baikal_admin_password").unwrap(), "(empty)");
    }

    #[test]
    fn test_flatten_for_ansible() {
        let toml_str = r#"
            [dns]
            domain = "example.com"

            [identity]
            ssh_port = 22022

            [baikal]
            baikal_admin_password = "secret"
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let flat = config.flatten_for_ansible();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
        assert_eq!(flat.get("baikal_admin_password").unwrap(), "secret");
    }
}
