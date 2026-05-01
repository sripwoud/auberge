use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub const TEMPLATE: &str = r#"admin_user_email = ""
admin_user_name = ""
admin_user_password = ""

baikal_admin_password = ""
baikal_subdomain = ""

bichon_encryption_password = ""
bichon_subdomain = ""
bichon_tailscale_ip = ""

blocky_subdomain = ""

grimmory_admin_password = ""
grimmory_admin_user = ""
grimmory_db_password = ""
grimmory_subdomain = ""

cloudflare_dns_api_token = ""

colporteur_feeds_password = ""
colporteur_freshrss_sync = false
colporteur_subdomain = ""

domain = ""

freshrss_subdomain = ""

hostname = ""

headscale_subdomain = ""

navidrome_subdomain = ""

hermes_exa_api_key = ""
hermes_llm_api_key = ""
hermes_llm_provider = ""
hermes_telegram_bot_token = ""

paperless_admin_password = ""
paperless_admin_user = ""
paperless_db_password = ""
paperless_secret_key = ""
paperless_subdomain = ""
paperless_tailscale_ip = ""

restic_password = ""
restic_repository = ""

ssh_port = 22022

tailscale_api_key = ""
tailscale_authkey = ""
tailscale_login_server = ""

tgtg_telegram_bot_token = ""

webdav_password = ""
webdav_subdomain = ""

yourls_admin_password = ""
yourls_admin_user = ""
yourls_api_signature = ""
yourls_cookiekey = ""
yourls_db_password = ""
yourls_subdomain = ""
"#;

const SENSITIVE_SUFFIXES: &[&str] = &["password", "key", "token", "secret", "cookie", "signature"];

const DEFAULT_TTL: u32 = 300;

/// Metadata describing a playbook's config requirements.
/// Acts as the Key Registry entry for a given playbook.
#[derive(Debug)]
pub struct PlaybookMeta {
    pub name: String,
    pub required_keys: Vec<String>,
}

impl PlaybookMeta {
    /// Look up the required config keys for a playbook by name.
    /// This is the Key Registry — the single authoritative source of
    /// which config keys each playbook needs.
    pub fn for_playbook(name: &str, tags: Option<&[String]>) -> Self {
        let mut required_keys: Vec<String> = match name {
            "bootstrap.yml" => vec![
                "admin_user_name".into(),
                "ssh_port".into(),
                "hostname".into(),
            ],
            "hardening.yml" => vec![],
            "infrastructure.yml" => vec![
                "admin_user_name".into(),
                "domain".into(),
                "tailscale_authkey".into(),
            ],
            "apps.yml" => vec![
                "admin_user_name".into(),
                "domain".into(),
                "cloudflare_dns_api_token".into(),
            ],
            "hermes.yml" => vec![
                "admin_user_name".into(),
                "domain".into(),
                "hermes_llm_provider".into(),
                "hermes_llm_api_key".into(),
                "hermes_telegram_bot_token".into(),
            ],
            _ => vec!["admin_user_name".into(), "domain".into()],
        };

        // For apps.yml, add tag-specific required keys
        if name == "apps.yml"
            && let Some(tags) = tags
        {
            for tag in tags {
                for key in tag_required_keys(tag) {
                    let key = key.to_string();
                    if !required_keys.contains(&key) {
                        required_keys.push(key);
                    }
                }
            }
        }

        PlaybookMeta {
            name: name.to_string(),
            required_keys,
        }
    }
}

fn tag_required_keys(tag: &str) -> &[&'static str] {
    match tag {
        "colporteur" => &["colporteur_subdomain"],
        "hermes" => &[
            "hermes_llm_provider",
            "hermes_llm_api_key",
            "hermes_telegram_bot_token",
        ],
        "tgtg" => &["tgtg_telegram_bot_token"],
        _ => &[],
    }
}

/// A validated snapshot of config variables ready for an Ansible run.
/// The only way to obtain a `Preflight` is via [`Config::preflight_for`],
/// which guarantees all required keys are present and resolved.
#[derive(Debug)]
pub struct Preflight {
    meta: PlaybookMeta,
    flat_vars: HashMap<String, String>,
}

impl Preflight {
    pub fn meta(&self) -> &PlaybookMeta {
        &self.meta
    }

    pub fn flat_vars(&self) -> &HashMap<String, String> {
        &self.flat_vars
    }
}

/// Merged configuration — the single source of truth for user settings.
/// Replaces both the old `UserConfig` and the old typed `Config`.
pub struct Config {
    path: PathBuf,
    values: toml::Table,
}

impl Config {
    // ── Constructors ──────────────────────────────────────────────────────────

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
        let values: toml::Table =
            toml::from_str(&contents).wrap_err("Failed to parse config.toml")?;
        Ok(Self { path, values })
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

    // ── Directory helpers ─────────────────────────────────────────────────────

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

    // ── Ergonomic accessors for ubiquitous keys ───────────────────────────────

    pub fn domain(&self) -> String {
        self.values
            .get("domain")
            .and_then(value_to_string)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    pub fn ttl(&self) -> u32 {
        self.values
            .get("default_ttl")
            .and_then(|v| {
                if let toml::Value::Integer(i) = v {
                    u32::try_from(*i).ok()
                } else {
                    None
                }
            })
            .unwrap_or(DEFAULT_TTL)
    }

    // ── Generic key accessors ─────────────────────────────────────────────────

    pub fn keys(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).and_then(value_to_string)
    }

    /// Returns the resolved value for `key`, executing shell commands for
    /// values prefixed with `!`.  Suitable for sensitive keys stored via
    /// secret managers (e.g. `restic_password = "!pass auberge/restic"`).
    pub fn get_secret(&self, key: &str) -> Result<Option<String>> {
        self.get_resolved(key)
    }

    pub fn get_resolved(&self, key: &str) -> Result<Option<String>> {
        match self.get(key) {
            Some(v) => resolve_value(&v)
                .wrap_err_with(|| format!("Failed to resolve config key '{key}'"))
                .map(Some),
            None => Ok(None),
        }
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        self.values
            .insert(key.to_string(), toml::Value::String(value.to_string()));
        self.save()
    }

    pub fn remove(&mut self, key: &str) -> Result<bool> {
        if self.values.remove(key).is_none() {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    // ── Display helpers ───────────────────────────────────────────────────────

    pub fn keys_redacted(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for (key, val) in &self.values {
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

    // ── Validation ────────────────────────────────────────────────────────────

    /// Returns the list of keys that are missing or empty.
    pub fn validate_required(&self, keys: &[&str]) -> Vec<String> {
        keys.iter()
            .filter(|&&key| match self.values.get(key) {
                None => true,
                Some(toml::Value::String(s)) => s.trim().is_empty(),
                _ => false,
            })
            .map(|&k| k.to_string())
            .collect()
    }

    pub fn validate_required_resolved(&self, keys: &[&str]) -> Result<()> {
        let missing = self.validate_required(keys);
        if !missing.is_empty() {
            eyre::bail!("Missing required config values: {}", missing.join(", "));
        }
        for &key in keys {
            self.get_resolved(key)?;
        }
        Ok(())
    }

    /// Validate that all required keys in `meta` are present and resolved,
    /// then return a flat map suitable for passing to Ansible.
    pub fn validate_for(&self, meta: &PlaybookMeta) -> Result<()> {
        let keys: Vec<&str> = meta.required_keys.iter().map(String::as_str).collect();
        self.validate_required_resolved(&keys)
    }

    // ── Ansible integration ───────────────────────────────────────────────────

    pub fn flatten_for_ansible(&self) -> HashMap<String, String> {
        flatten_toml(&self.values)
    }

    /// Build a [`Preflight`] for `playbook`, validating all required keys.
    ///
    /// This is the **only** constructor for `Preflight`.  It looks up the
    /// playbook's requirements in the Key Registry (`PlaybookMeta`), validates
    /// the config, and returns a capability value that unlocks `AnsibleRunner`.
    pub fn preflight_for(&self, playbook: &str, tags: Option<&[String]>) -> Result<Preflight> {
        let meta = PlaybookMeta::for_playbook(playbook, tags);
        self.validate_for(&meta)?;
        let flat_vars = self.flatten_for_ansible();
        Ok(Preflight { meta, flat_vars })
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn save(&self) -> Result<()> {
        let contents = toml::to_string_pretty(&self.values).wrap_err("Failed to serialize TOML")?;
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

fn resolve_value(v: &str) -> Result<String> {
    if let Some(rest) = v.strip_prefix("!!") {
        return Ok(format!("!{rest}"));
    }
    if let Some(cmd) = v.strip_prefix('!') {
        use std::process::Stdio;
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .wrap_err("Failed to execute shell command")?;
        if !output.status.success() {
            let code = output
                .status
                .code()
                .map_or("signal".to_string(), |c| c.to_string());
            eyre::bail!("Shell command failed (exit {code})");
        }
        let stdout =
            String::from_utf8(output.stdout).wrap_err("Shell command output is not valid UTF-8")?;
        let resolved = stdout.trim().to_string();
        if resolved.is_empty() {
            eyre::bail!("Shell command produced empty output");
        }
        return Ok(resolved);
    }
    Ok(v.to_string())
}

fn flatten_toml(table: &toml::Table) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for (key, value) in table {
        match value {
            toml::Value::Table(inner) => result.extend(flatten_toml(inner)),
            other => {
                if let Some(s) = value_to_string(other)
                    && let Ok(resolved) = resolve_value(&s)
                {
                    result.insert(key.clone(), resolved);
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

    fn make_config(toml_str: &str) -> Config {
        let values: toml::Table = toml::from_str(toml_str).unwrap();
        Config {
            path: PathBuf::from("/tmp/fake"),
            values,
        }
    }

    // ── Ergonomic accessors ───────────────────────────────────────────────────

    #[test]
    fn test_domain_accessor() {
        let config = make_config(r#"domain = "example.com""#);
        assert_eq!(config.domain(), "example.com");
    }

    #[test]
    fn test_domain_accessor_trims_whitespace() {
        let config = make_config(r#"domain = " example.com ""#);
        assert_eq!(config.domain(), "example.com");
    }

    #[test]
    fn test_domain_accessor_empty_when_missing() {
        let config = make_config("");
        assert_eq!(config.domain(), "");
    }

    #[test]
    fn test_ttl_default() {
        let config = make_config(r#"domain = "example.com""#);
        assert_eq!(config.ttl(), 300);
    }

    #[test]
    fn test_ttl_custom() {
        let config = make_config("domain = \"example.com\"\ndefault_ttl = 600");
        assert_eq!(config.ttl(), 600);
    }

    // ── Generic accessors ─────────────────────────────────────────────────────

    #[test]
    fn test_keys_returns_all_key_names() {
        let config = make_config(
            r#"
            domain = "example.com"
            admin_user_name = "alice"
            ssh_port = 22022
        "#,
        );
        let mut keys = config.keys();
        keys.sort();
        assert_eq!(keys, vec!["admin_user_name", "domain", "ssh_port"]);
    }

    #[test]
    fn test_get_nonexistent_key() {
        let config = make_config(TEMPLATE);
        assert!(config.get("nonexistent_key").is_none());
    }

    // ── keys_redacted ─────────────────────────────────────────────────────────

    #[test]
    fn test_keys_redacted() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            cloudflare_dns_api_token = "secret123"
            baikal_admin_password = ""
        "#,
        );
        use std::collections::BTreeMap;
        let items = config.keys_redacted();
        let map: BTreeMap<_, _> = items.into_iter().collect();
        assert_eq!(map.get("admin_user_name").unwrap(), "alice");
        assert_eq!(map.get("cloudflare_dns_api_token").unwrap(), "****");
        assert_eq!(map.get("baikal_admin_password").unwrap(), "(empty)");
    }

    // ── validate_required ─────────────────────────────────────────────────────

    #[test]
    fn test_validate_required_catches_empty_strings() {
        let config = make_config(
            r#"
            domain = "example.com"
            admin_user_name = ""
            ssh_port = 22022
        "#,
        );
        let missing = config.validate_required(&["domain", "admin_user_name", "ssh_port"]);
        assert_eq!(missing, vec!["admin_user_name"]);
    }

    #[test]
    fn test_validate_required_catches_missing_keys() {
        let config = make_config(r#"domain = "example.com""#);
        let missing = config.validate_required(&["domain", "admin_user_name"]);
        assert_eq!(missing, vec!["admin_user_name"]);
    }

    #[test]
    fn test_validate_required_catches_whitespace_only_values() {
        let config = make_config(
            r#"
            domain = "  "
            admin_user_name = "	"
            ssh_port = 22022
        "#,
        );
        let missing = config.validate_required(&["domain", "admin_user_name", "ssh_port"]);
        assert_eq!(missing, vec!["domain", "admin_user_name"]);
    }

    #[test]
    fn test_validate_required_returns_empty_when_all_set() {
        let config = make_config(
            r#"
            domain = "example.com"
            admin_user_name = "alice"
        "#,
        );
        let missing = config.validate_required(&["domain", "admin_user_name"]);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_validate_required_resolved_passes_when_all_set() {
        let config = make_config(
            r#"
            domain = "example.com"
            admin_user_name = "alice"
        "#,
        );
        assert!(
            config
                .validate_required_resolved(&["domain", "admin_user_name"])
                .is_ok()
        );
    }

    #[test]
    fn test_validate_required_resolved_fails_on_missing_key() {
        let config = make_config(r#"domain = "example.com""#);
        let err = config
            .validate_required_resolved(&["domain", "admin_user_name"])
            .unwrap_err();
        assert!(err.to_string().contains("admin_user_name"));
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_required_resolved_fails_on_broken_shell_command() {
        let config = make_config(
            r#"
            domain = "example.com"
            bot_token = "!false"
        "#,
        );
        let err = config
            .validate_required_resolved(&["domain", "bot_token"])
            .unwrap_err();
        assert!(err.to_string().contains("bot_token"));
    }

    // ── flatten_for_ansible ───────────────────────────────────────────────────

    #[test]
    fn test_flatten_toml() {
        let config = make_config(
            r#"
            domain = "example.com"
            ssh_port = 22022
            admin_user_name = "alice"
        "#,
        );
        let flat = flatten_toml(&config.values);
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
        assert_eq!(flat.get("admin_user_name").unwrap(), "alice");
    }

    #[test]
    fn test_flatten_for_ansible() {
        let config = make_config(
            r#"
            domain = "example.com"
            ssh_port = 22022
            baikal_admin_password = "secret"
        "#,
        );
        let flat = config.flatten_for_ansible();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
        assert_eq!(flat.get("baikal_admin_password").unwrap(), "secret");
    }

    #[cfg(unix)]
    #[test]
    fn test_flatten_for_ansible_resolves_command() {
        let config = make_config(
            r#"
            domain = "example.com"
            baikal_admin_password = "!echo cmdpassword"
        "#,
        );
        let flat = config.flatten_for_ansible();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("baikal_admin_password").unwrap(), "cmdpassword");
    }

    #[test]
    fn test_flatten_for_ansible_resolves_escaped_bang() {
        let config = make_config(
            r#"
            domain = "example.com"
            baikal_admin_password = "!!literal"
        "#,
        );
        let flat = config.flatten_for_ansible();
        assert_eq!(flat.get("baikal_admin_password").unwrap(), "!literal");
    }

    #[cfg(unix)]
    #[test]
    fn test_flatten_for_ansible_skips_failed_shell_commands() {
        let config = make_config(
            r#"
            domain = "example.com"
            broken_key = "!false"
        "#,
        );
        let flat = config.flatten_for_ansible();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert!(flat.get("broken_key").is_none());
    }

    // ── get_resolved / get_secret ─────────────────────────────────────────────

    #[test]
    fn test_get_resolved_plain_value() {
        let config = make_config(
            r#"
            domain = "example.com"
            restic_password = "secret123"
        "#,
        );
        assert_eq!(
            config.get_resolved("domain").unwrap().unwrap(),
            "example.com"
        );
        assert_eq!(
            config.get_resolved("restic_password").unwrap().unwrap(),
            "secret123"
        );
        assert!(config.get_resolved("nonexistent").unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_get_resolved_shell_command() {
        let config = make_config(r#"restic_password = "!echo resolved_secret""#);
        assert_eq!(
            config.get_resolved("restic_password").unwrap().unwrap(),
            "resolved_secret"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_resolved_escaped_bang() {
        let config = make_config(r#"value = "!!literal-bang""#);
        assert_eq!(
            config.get_resolved("value").unwrap().unwrap(),
            "!literal-bang"
        );
    }

    // ── resolve_value ─────────────────────────────────────────────────────────

    #[test]
    fn test_resolve_value_plain_string() {
        assert_eq!(resolve_value("hello").unwrap(), "hello");
        assert_eq!(resolve_value("secret123").unwrap(), "secret123");
        assert_eq!(resolve_value("").unwrap(), "");
    }

    #[test]
    fn test_resolve_value_escaped_bang() {
        assert_eq!(resolve_value("!!literal-bang").unwrap(), "!literal-bang");
        assert_eq!(resolve_value("!!pass foo").unwrap(), "!pass foo");
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_value_shell_command() {
        let result = resolve_value("!echo mysecret").unwrap();
        assert_eq!(result, "mysecret");
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_value_shell_command_trims_whitespace() {
        let result = resolve_value("!printf '  trimmed  '").unwrap();
        assert_eq!(result, "trimmed");
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_value_shell_command_nonzero_exit_fails() {
        let err = resolve_value("!false").unwrap_err();
        assert!(err.to_string().contains("Shell command failed"));
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_value_shell_command_empty_output_fails() {
        let err = resolve_value("!true").unwrap_err();
        assert!(err.to_string().contains("empty output"));
    }

    // ── round-trip / file operations ──────────────────────────────────────────

    #[test]
    fn test_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = Config {
            path: config_path.clone(),
            values: toml::from_str(TEMPLATE).unwrap(),
        };

        config.set("admin_user_name", "bob").unwrap();
        assert_eq!(config.get("admin_user_name").unwrap(), "bob");

        let reloaded_contents = fs::read_to_string(&config_path).unwrap();
        let reloaded_values: toml::Table = toml::from_str(&reloaded_contents).unwrap();
        let reloaded = Config {
            path: config_path,
            values: reloaded_values,
        };
        assert_eq!(reloaded.get("admin_user_name").unwrap(), "bob");
    }

    #[test]
    fn test_permissions_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();

        Config::enforce_permissions(&config_path).unwrap();
        let perms = fs::metadata(&config_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn test_template_parses() {
        let values: toml::Table = toml::from_str(TEMPLATE).unwrap();
        assert!(values.contains_key("domain"));
        assert!(values.contains_key("admin_user_name"));
        assert!(values.contains_key("cloudflare_dns_api_token"));
        assert!(values.contains_key("tailscale_authkey"));
    }

    #[test]
    fn test_remove_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, TEMPLATE).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = Config {
            path: config_path,
            values: toml::from_str(TEMPLATE).unwrap(),
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

        let mut config = Config {
            path: config_path,
            values: toml::from_str(TEMPLATE).unwrap(),
        };

        assert!(config.get("brand_new_key").is_none());
        config.set("brand_new_key", "hello").unwrap();
        assert_eq!(config.get("brand_new_key").unwrap(), "hello");
    }

    // ── PlaybookMeta / KeyRegistry ────────────────────────────────────────────

    #[test]
    fn test_playbook_meta_bootstrap() {
        let meta = PlaybookMeta::for_playbook("bootstrap.yml", None);
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"ssh_port".to_string()));
        assert!(meta.required_keys.contains(&"hostname".to_string()));
    }

    #[test]
    fn test_playbook_meta_infrastructure() {
        let meta = PlaybookMeta::for_playbook("infrastructure.yml", None);
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
        assert!(
            meta.required_keys
                .contains(&"tailscale_authkey".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_apps() {
        let meta = PlaybookMeta::for_playbook("apps.yml", None);
        assert!(
            meta.required_keys
                .contains(&"cloudflare_dns_api_token".to_string())
        );
        assert!(
            !meta
                .required_keys
                .contains(&"colporteur_subdomain".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_apps_with_colporteur_tag() {
        let tags = vec!["colporteur".to_string()];
        let meta = PlaybookMeta::for_playbook("apps.yml", Some(&tags));
        assert!(
            meta.required_keys
                .contains(&"cloudflare_dns_api_token".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"colporteur_subdomain".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_apps_with_hermes_tag() {
        let tags = vec!["hermes".to_string()];
        let meta = PlaybookMeta::for_playbook("apps.yml", Some(&tags));
        assert!(
            meta.required_keys
                .contains(&"cloudflare_dns_api_token".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_llm_provider".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_llm_api_key".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_telegram_bot_token".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_apps_with_tgtg_tag() {
        let tags = vec!["tgtg".to_string()];
        let meta = PlaybookMeta::for_playbook("apps.yml", Some(&tags));
        assert!(
            meta.required_keys
                .contains(&"tgtg_telegram_bot_token".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_apps_with_unrelated_tag() {
        let tags = vec!["paperless".to_string()];
        let meta = PlaybookMeta::for_playbook("apps.yml", Some(&tags));
        assert!(
            !meta
                .required_keys
                .contains(&"colporteur_subdomain".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_ignores_tags_for_non_apps_playbooks() {
        let tags = vec!["colporteur".to_string()];
        let meta = PlaybookMeta::for_playbook("infrastructure.yml", Some(&tags));
        assert!(
            !meta
                .required_keys
                .contains(&"colporteur_subdomain".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_hardening_is_empty() {
        let meta = PlaybookMeta::for_playbook("hardening.yml", None);
        assert!(meta.required_keys.is_empty());
    }

    #[test]
    fn test_playbook_meta_hermes() {
        let meta = PlaybookMeta::for_playbook("hermes.yml", None);
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
        assert!(
            meta.required_keys
                .contains(&"hermes_llm_provider".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_llm_api_key".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_telegram_bot_token".to_string())
        );
    }

    #[test]
    fn test_playbook_meta_unknown_playbook_returns_defaults() {
        let meta = PlaybookMeta::for_playbook("custom.yml", None);
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
    }

    // ── preflight_for ─────────────────────────────────────────────────────────

    #[test]
    fn test_preflight_for_succeeds_when_all_keys_present() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            tailscale_authkey = "tskey-abc123"
        "#,
        );
        let result = config.preflight_for("infrastructure.yml", None);
        assert!(result.is_ok());
        let preflight = result.unwrap();
        assert_eq!(preflight.meta().name, "infrastructure.yml");
        assert_eq!(preflight.flat_vars().get("domain").unwrap(), "example.com");
    }

    #[test]
    fn test_preflight_for_fails_when_required_key_missing() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
        "#,
        );
        let err = config
            .preflight_for("infrastructure.yml", None)
            .unwrap_err();
        assert!(
            err.to_string().contains("tailscale_authkey"),
            "error should mention missing key: {}",
            err
        );
    }

    #[test]
    fn test_preflight_for_fails_when_required_key_empty() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            tailscale_authkey = ""
        "#,
        );
        let err = config
            .preflight_for("infrastructure.yml", None)
            .unwrap_err();
        assert!(
            err.to_string().contains("tailscale_authkey"),
            "error should mention empty key: {}",
            err
        );
    }

    #[test]
    fn test_preflight_for_hardening_requires_no_keys() {
        // hardening.yml has no required keys — should succeed with empty config
        let config = make_config("");
        let result = config.preflight_for("hardening.yml", None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_preflight_for_flat_vars_contains_all_config() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            tailscale_authkey = "tskey-abc"
            ssh_port = 22022
        "#,
        );
        let preflight = config.preflight_for("infrastructure.yml", None).unwrap();
        let flat = preflight.flat_vars();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
    }

    #[test]
    fn test_preflight_for_apps_with_tag_validates_tag_keys() {
        let tags = vec!["colporteur".to_string()];
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            cloudflare_dns_api_token = "cftoken"
        "#,
        );
        // Missing colporteur_subdomain
        let err = config.preflight_for("apps.yml", Some(&tags)).unwrap_err();
        assert!(
            err.to_string().contains("colporteur_subdomain"),
            "error should mention missing tag key: {}",
            err
        );
    }

    #[test]
    fn test_preflight_for_secret_keys_present_in_flat_vars() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            tailscale_authkey = "tskey-supersecret"
        "#,
        );
        let preflight = config.preflight_for("infrastructure.yml", None).unwrap();
        // flat_vars contains the actual (unredacted) secret value for ansible
        assert_eq!(
            preflight.flat_vars().get("tailscale_authkey").unwrap(),
            "tskey-supersecret"
        );
    }
}
