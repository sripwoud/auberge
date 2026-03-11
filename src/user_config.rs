use eyre::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct UserConfig {
    path: PathBuf,
    table: toml::Table,
}

const TEMPLATE: &str = r#"admin_user_email = ""
admin_user_name = ""

baikal_admin_password = ""
baikal_subdomain = ""

bichon_encryption_password = ""
bichon_subdomain = ""
bichon_tailscale_ip = ""

blocky_subdomain = ""

booklore_admin_password = ""
booklore_admin_user = ""
booklore_db_password = ""
booklore_subdomain = ""

cloudflare_dns_api_token = ""

colporteur_feeds_password = ""
colporteur_subdomain = ""

domain = ""

freshrss_subdomain = ""

headscale_subdomain = ""

navidrome_subdomain = ""

openclaw_claude_ai_session_key = ""
openclaw_claude_web_cookie = ""
openclaw_claude_web_session_key = ""
openclaw_gateway_token = ""

paperless_admin_password = ""
paperless_admin_user = ""
paperless_db_password = ""
paperless_secret_key = ""
paperless_subdomain = ""
paperless_tailscale_ip = ""

primary_domain = ""

restic_password = ""
restic_repository = ""

ssh_port = 22022

tailscale_api_key = ""
tailscale_authkey = ""
tailscale_login_server = ""

vdirsyncer_baikal_calendar_name = ""
vdirsyncer_icloud_calendar_id = ""
vdirsyncer_icloud_password = ""
vdirsyncer_icloud_url = ""
vdirsyncer_icloud_username = ""

webdav_password = ""
webdav_subdomain = ""

yourls_admin_password = ""
yourls_admin_user = ""
yourls_api_signature = ""
yourls_cookiekey = ""
yourls_db_password = ""
yourls_subdomain = ""

zone_id = ""
"#;

const SENSITIVE_SUFFIXES: &[&str] = &["password", "key", "token", "secret", "cookie", "signature"];

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

    pub fn keys(&self) -> Vec<String> {
        self.table.keys().cloned().collect()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.table.get(key).and_then(value_to_string)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        self.table
            .insert(key.to_string(), toml::Value::String(value.to_string()));
        self.save()
    }

    pub fn remove(&mut self, key: &str) -> Result<bool> {
        if self.table.remove(key).is_none() {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    pub fn keys_redacted(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for (key, val) in &self.table {
            let is_sensitive = SENSITIVE_SUFFIXES.iter().any(|s| key.contains(s));
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
        result
    }

    pub fn validate_required(&self, keys: &[&str]) -> Vec<String> {
        keys.iter()
            .filter(|&&key| match self.table.get(key) {
                None => true,
                Some(toml::Value::String(s)) => s.trim().is_empty(),
                _ => false,
            })
            .map(|&k| k.to_string())
            .collect()
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
    fn test_keys_returns_all_key_names() {
        let toml_str = r#"
            domain = "example.com"
            admin_user_name = "alice"
            ssh_port = 22022
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let mut keys = config.keys();
        keys.sort();
        assert_eq!(keys, vec!["admin_user_name", "domain", "ssh_port"]);
    }

    #[test]
    fn test_flatten_toml() {
        let toml_str = r#"
            domain = "example.com"
            ssh_port = 22022
            admin_user_name = "alice"
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let flat = flatten_toml(&table);
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
        assert_eq!(flat.get("admin_user_name").unwrap(), "alice");
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

        config.set("admin_user_name", "bob").unwrap();
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
        assert!(table.contains_key("domain"));
        assert!(table.contains_key("admin_user_name"));
        assert!(table.contains_key("cloudflare_dns_api_token"));
        assert!(table.contains_key("tailscale_authkey"));
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
    fn test_set_upserts_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = UserConfig {
            path: config_path,
            table: toml::from_str(TEMPLATE).unwrap(),
        };

        assert!(config.get("brand_new_key").is_none());
        config.set("brand_new_key", "hello").unwrap();
        assert_eq!(config.get("brand_new_key").unwrap(), "hello");
    }

    #[test]
    fn test_keys_redacted() {
        let toml_str = r#"
            admin_user_name = "alice"
            cloudflare_dns_api_token = "secret123"
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
    fn test_validate_required_catches_empty_strings() {
        let toml_str = r#"
            domain = "example.com"
            admin_user_name = ""
            ssh_port = 22022
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let missing = config.validate_required(&["domain", "admin_user_name", "ssh_port"]);
        assert_eq!(missing, vec!["admin_user_name"]);
    }

    #[test]
    fn test_validate_required_catches_missing_keys() {
        let toml_str = r#"
            domain = "example.com"
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let missing = config.validate_required(&["domain", "admin_user_name"]);
        assert_eq!(missing, vec!["admin_user_name"]);
    }

    #[test]
    fn test_validate_required_catches_whitespace_only_values() {
        let toml_str = r#"
            domain = "  "
            admin_user_name = "	"
            ssh_port = 22022
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let missing = config.validate_required(&["domain", "admin_user_name", "ssh_port"]);
        assert_eq!(missing, vec!["domain", "admin_user_name"]);
    }

    #[test]
    fn test_validate_required_returns_empty_when_all_set() {
        let toml_str = r#"
            domain = "example.com"
            admin_user_name = "alice"
        "#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let config = UserConfig {
            path: PathBuf::from("/tmp/fake"),
            table,
        };
        let missing = config.validate_required(&["domain", "admin_user_name"]);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_flatten_for_ansible() {
        let toml_str = r#"
            domain = "example.com"
            ssh_port = 22022
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
