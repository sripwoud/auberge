use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const SENSITIVE_SUFFIXES: &[&str] = &["password", "key", "token", "secret", "cookie", "signature"];

/// Reserved top-level table holding per-Host overrides: `[hosts.<name>]`
/// answers a key for that Host only (ADR-0058). Never flattened fleet-wide.
pub const HOSTS_TABLE: &str = "hosts";

const DEFAULT_TTL: u32 = 300;

/// A validated snapshot of config variables ready for an Ansible run.
/// The only way to obtain a `Preflight` is via [`Config::preflight_with_keys`],
/// which guarantees all required keys are present and resolved.
#[derive(Debug)]
pub struct Preflight {
    flat_vars: HashMap<String, String>,
}

impl Preflight {
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
                "Config file not found at {p}. Generate one with `auberge config init --output {p}`.",
                p = path.display()
            );
        }
        let contents = fs::read_to_string(&path)
            .wrap_err_with(|| format!("Failed to read {}", path.display()))?;
        let values: toml::Table =
            toml::from_str(&contents).wrap_err("Failed to parse config.toml")?;
        Ok(Self { path, values })
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

    pub fn get_resolved(&self, key: &str) -> Result<Option<String>> {
        match self.get(key) {
            Some(v) => resolve_value(&v)
                .wrap_err_with(|| format!("Failed to resolve config key '{key}'"))
                .map(Some),
            None => Ok(None),
        }
    }

    // ── Host-scoped view (ADR-0058) ───────────────────────────────────────────

    /// The `[hosts.<name>]` override table for one Host, when config carries one.
    fn host_overrides(&self, host: &str) -> Option<&toml::Table> {
        self.values
            .get(HOSTS_TABLE)?
            .as_table()?
            .get(host)?
            .as_table()
    }

    /// The value `key` takes for `host`: the Host's override when its table
    /// carries the key — a blank override is how a Host withdraws a fleet-wide
    /// answer — else the top-level value. `None` is the fleet-wide view.
    fn effective(&self, key: &str, host: Option<&str>) -> Option<&toml::Value> {
        if let Some(h) = host
            && let Some(v) = self.host_overrides(h).and_then(|t| t.get(key))
        {
            return Some(v);
        }
        self.values.get(key)
    }

    pub fn get_for_host(&self, key: &str, host: Option<&str>) -> Option<String> {
        self.effective(key, host).and_then(value_to_string)
    }

    /// The Host names the reserved table carries. A name no host answers to is
    /// a fail-open typo — Preflight checks these against the roster.
    pub fn host_override_names(&self) -> Vec<String> {
        self.values
            .get(HOSTS_TABLE)
            .and_then(|v| v.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Move `[hosts.<old>]` to `[hosts.<new>]` so a host rename does not
    /// orphan the per-Host answers keyed on the name. `Ok(false)` when there
    /// is nothing to move — the already-moved state a rerun converges through
    /// (ADR-0024). Both existing is a conflict the caller must resolve.
    pub fn rename_host_overrides(&mut self, old: &str, new: &str) -> Result<bool> {
        let Some(tables) = self
            .values
            .get_mut(HOSTS_TABLE)
            .and_then(|v| v.as_table_mut())
        else {
            return Ok(false);
        };
        if tables.contains_key(old) && tables.contains_key(new) {
            eyre::bail!(
                "config.toml holds both [hosts.{old}] and [hosts.{new}]; merge them before renaming"
            );
        }
        let Some(entry) = tables.remove(old) else {
            return Ok(false);
        };
        tables.insert(new.to_string(), entry);
        self.save()?;
        Ok(true)
    }

    pub fn bichon_extra_excluded_folders(&self, account_email: &str) -> Vec<String> {
        self.values
            .get("bichon")
            .and_then(toml::Value::as_table)
            .and_then(|b| b.get("account_overrides"))
            .and_then(toml::Value::as_table)
            .and_then(|a| a.get(account_email))
            .and_then(toml::Value::as_table)
            .and_then(|entry| entry.get("extra_excluded_folders"))
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn bichon_host_base_url(&self, host_name: &str) -> Option<String> {
        self.values
            .get("bichon")
            .and_then(toml::Value::as_table)
            .and_then(|b| b.get("hosts"))
            .and_then(toml::Value::as_table)
            .and_then(|hosts| hosts.get(host_name))
            .and_then(toml::Value::as_table)
            .and_then(|entry| entry.get("base_url"))
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.trim_end_matches('/').to_string())
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if key == HOSTS_TABLE {
            eyre::bail!(
                "'{key}' is the reserved per-Host override table (ADR-0058); \
                 edit {} directly.",
                self.path.display()
            );
        }
        if key.contains('.') {
            eyre::bail!(
                "'{key}' looks like a nested key, which `set` cannot write. \
                 Edit {} directly (e.g. a [hosts.<name>] table).",
                self.path.display()
            );
        }
        self.values
            .insert(key.to_string(), toml::Value::String(value.to_string()));
        self.save()
    }

    pub fn remove(&mut self, key: &str) -> Result<bool> {
        if key == HOSTS_TABLE {
            eyre::bail!(
                "'{key}' is the reserved per-Host override table (ADR-0058); \
                 removing it would drop every host's overrides. Edit {} directly.",
                self.path.display()
            );
        }
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
            if key == HOSTS_TABLE {
                let Some(tables) = val.as_table() else {
                    continue;
                };
                for (host, overrides) in tables {
                    let Some(entries) = overrides.as_table() else {
                        continue;
                    };
                    for (k, v) in entries {
                        result.push((format!("hosts.{host}.{k}"), redacted_display(k, v)));
                    }
                }
                continue;
            }
            result.push((key.clone(), redacted_display(key, val)));
        }
        result
    }

    // ── Validation ────────────────────────────────────────────────────────────

    /// Returns the list of keys that are missing or empty.
    pub fn validate_required(&self, keys: &[&str], host: Option<&str>) -> Vec<String> {
        keys.iter()
            .filter(|&&key| match self.effective(key, host) {
                None => true,
                Some(toml::Value::String(s)) => s.trim().is_empty(),
                _ => false,
            })
            .map(|&k| k.to_string())
            .collect()
    }

    pub fn validate_required_resolved(&self, keys: &[&str], host: Option<&str>) -> Result<()> {
        let missing = self.validate_required(keys, host);
        if !missing.is_empty() {
            eyre::bail!("Missing required config values: {}", missing.join(", "));
        }
        for &key in keys {
            if let Some(value) = self.get_for_host(key, host) {
                resolve_value(&value)
                    .wrap_err_with(|| format!("Failed to resolve config key '{key}'"))?;
            }
        }
        Ok(())
    }

    // ── Ansible integration ───────────────────────────────────────────────────

    /// Every config value as a flat `name -> value` map for one Host's run:
    /// the top level minus the reserved `hosts` table, with the Host's
    /// `[hosts.<name>]` overrides merged on top (ADR-0058).
    ///
    /// Top-level values that fail to resolve are dropped (the run may never
    /// read them); an override that fails is an error — dropping it would
    /// silently fall back to the fleet-wide value the Host meant to replace.
    pub fn flatten_for_ansible(&self, host: Option<&str>) -> Result<HashMap<String, String>> {
        let mut flat = HashMap::new();
        for (key, value) in &self.values {
            if key == HOSTS_TABLE {
                continue;
            }
            flatten_entry(key, value, &mut flat);
        }
        let Some(h) = host else {
            return Ok(flat);
        };
        let Some(overrides) = self.host_overrides(h) else {
            return Ok(flat);
        };
        for (key, value) in overrides {
            match value {
                toml::Value::Table(inner) => flat.extend(flatten_toml(inner)),
                other => {
                    let raw = value_to_string(other).ok_or_else(|| {
                        eyre::eyre!("[hosts.{h}] override '{key}' is not a scalar value")
                    })?;
                    let resolved = resolve_value(&raw).wrap_err_with(|| {
                        format!("[hosts.{h}] override '{key}' failed to resolve")
                    })?;
                    flat.insert(key.clone(), resolved);
                }
            }
        }
        Ok(flat)
    }

    /// Build a [`Preflight`] from the keys a run's Playbook Metas declare.
    ///
    /// This is the **only** constructor for `Preflight`: it validates every key
    /// is present and resolvable, then returns the capability value that
    /// unlocks `AnsibleRunner`. Callers resolve the keys through
    /// [`crate::services::required_keys::preflight_for`], which reads the
    /// declarations off the Metas.
    pub fn preflight_with_keys(
        &self,
        required_keys: &[String],
        host: Option<&str>,
    ) -> Result<Preflight> {
        let keys: Vec<&str> = required_keys.iter().map(String::as_str).collect();
        self.validate_required_resolved(&keys, host)?;
        Ok(Preflight {
            flat_vars: self.flatten_for_ansible(host)?,
        })
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

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Construct a `Config` from a TOML string without touching the filesystem.
    /// Only available in test builds; used by unit tests in other modules.
    #[cfg(test)]
    pub fn from_toml_str(toml_str: &str) -> Result<Self> {
        let values: toml::Table =
            toml::from_str(toml_str).wrap_err("Failed to parse TOML in test fixture")?;
        Ok(Config {
            path: PathBuf::from("/tmp/fake"),
            values,
        })
    }
}

fn redacted_display(key: &str, val: &toml::Value) -> String {
    let is_sensitive = SENSITIVE_SUFFIXES.iter().any(|s| key.contains(s));
    if is_sensitive {
        return match val {
            toml::Value::String(s) if s.is_empty() => "(empty)".to_string(),
            toml::Value::String(_) => "****".to_string(),
            other => value_to_string(other).unwrap_or_default(),
        };
    }
    value_to_string(val).unwrap_or_default()
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
        flatten_entry(key, value, &mut result);
    }
    result
}

fn flatten_entry(key: &str, value: &toml::Value, result: &mut HashMap<String, String>) {
    match value {
        toml::Value::Table(inner) => result.extend(flatten_toml(inner)),
        other => {
            if let Some(s) = value_to_string(other)
                && let Ok(resolved) = resolve_value(&s)
            {
                result.insert(key.to_string(), resolved);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn make_config(toml_str: &str) -> Config {
        Config::from_toml_str(toml_str).unwrap()
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
        let config = make_config(r#"domain = "example.com""#);
        assert!(config.get("nonexistent_key").is_none());
    }

    #[test]
    fn test_bichon_extra_excluded_folders() {
        let config = make_config(
            r#"
            [bichon.account_overrides."me@example.com"]
            extra_excluded_folders = ["Newsletters", "Receipts/2019"]
        "#,
        );
        assert_eq!(
            config.bichon_extra_excluded_folders("me@example.com"),
            vec!["Newsletters".to_string(), "Receipts/2019".to_string()]
        );
        assert!(
            config
                .bichon_extra_excluded_folders("missing@example.com")
                .is_empty()
        );
    }

    #[test]
    fn test_bichon_host_base_url() {
        let config = make_config(
            r#"
            [bichon.hosts.auberge]
            base_url = "https://bichon.auberge.example.com/"

            [bichon.hosts.staging]
            base_url = ""
        "#,
        );
        assert_eq!(
            config.bichon_host_base_url("auberge"),
            Some("https://bichon.auberge.example.com".to_string())
        );
        assert!(config.bichon_host_base_url("staging").is_none());
        assert!(config.bichon_host_base_url("missing").is_none());
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
        let missing = config.validate_required(&["domain", "admin_user_name", "ssh_port"], None);
        assert_eq!(missing, vec!["admin_user_name"]);
    }

    #[test]
    fn test_validate_required_catches_missing_keys() {
        let config = make_config(r#"domain = "example.com""#);
        let missing = config.validate_required(&["domain", "admin_user_name"], None);
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
        let missing = config.validate_required(&["domain", "admin_user_name", "ssh_port"], None);
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
        let missing = config.validate_required(&["domain", "admin_user_name"], None);
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
                .validate_required_resolved(&["domain", "admin_user_name"], None)
                .is_ok()
        );
    }

    #[test]
    fn test_validate_required_resolved_fails_on_missing_key() {
        let config = make_config(r#"domain = "example.com""#);
        let err = config
            .validate_required_resolved(&["domain", "admin_user_name"], None)
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
            .validate_required_resolved(&["domain", "bot_token"], None)
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
        let flat = config.flatten_for_ansible(None).unwrap();
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
        let flat = config.flatten_for_ansible(None).unwrap();
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
        let flat = config.flatten_for_ansible(None).unwrap();
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
        let flat = config.flatten_for_ansible(None).unwrap();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert!(flat.get("broken_key").is_none());
    }

    // ── get_resolved ──────────────────────────────────────────────────────────

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

    const SAMPLE_TOML: &str = r#"
admin_user_name = ""
domain = ""
ssh_port = 22022
"#;

    #[test]
    fn test_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, SAMPLE_TOML).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = Config {
            path: config_path.clone(),
            values: toml::from_str(SAMPLE_TOML).unwrap(),
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
        fs::write(&config_path, SAMPLE_TOML).unwrap();

        Config::enforce_permissions(&config_path).unwrap();
        let perms = fs::metadata(&config_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn test_remove_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, SAMPLE_TOML).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = Config {
            path: config_path,
            values: toml::from_str(SAMPLE_TOML).unwrap(),
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
        fs::write(&config_path, SAMPLE_TOML).unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();

        let mut config = Config {
            path: config_path,
            values: toml::from_str(SAMPLE_TOML).unwrap(),
        };

        assert!(config.get("brand_new_key").is_none());
        config.set("brand_new_key", "hello").unwrap();
        assert_eq!(config.get("brand_new_key").unwrap(), "hello");
    }

    // ── preflight_with_keys ───────────────────────────────────────────────────

    #[test]
    fn test_preflight_succeeds_when_all_keys_present() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            tailscale_authkey = "tskey-abc123"
        "#,
        );
        let keys = vec!["admin_user_name".to_string(), "domain".to_string()];
        let preflight = config.preflight_with_keys(&keys, None).unwrap();
        assert_eq!(preflight.flat_vars().get("domain").unwrap(), "example.com");
    }

    #[test]
    fn test_preflight_fails_when_required_key_missing() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
        "#,
        );
        let err = config
            .preflight_with_keys(&["tailscale_authkey".to_string()], None)
            .unwrap_err();
        assert!(
            err.to_string().contains("tailscale_authkey"),
            "error should mention missing key: {err}"
        );
    }

    #[test]
    fn test_preflight_fails_when_required_key_empty() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            tailscale_authkey = ""
        "#,
        );
        let err = config
            .preflight_with_keys(&["tailscale_authkey".to_string()], None)
            .unwrap_err();
        assert!(
            err.to_string().contains("tailscale_authkey"),
            "error should mention empty key: {err}"
        );
    }

    #[test]
    fn test_preflight_with_no_keys_accepts_an_empty_config() {
        let config = make_config("");
        assert!(config.preflight_with_keys(&[], None).is_ok());
    }

    #[test]
    fn test_preflight_flat_vars_contains_all_config_not_just_required() {
        let config = make_config(
            r#"
            admin_user_name = "alice"
            domain = "example.com"
            ssh_port = 22022
        "#,
        );
        let preflight = config
            .preflight_with_keys(&["domain".to_string()], None)
            .unwrap();
        let flat = preflight.flat_vars();
        assert_eq!(flat.get("domain").unwrap(), "example.com");
        assert_eq!(flat.get("ssh_port").unwrap(), "22022");
    }

    #[test]
    fn test_preflight_secret_keys_present_unredacted_in_flat_vars() {
        let config = make_config(
            r#"
            tailscale_authkey = "tskey-supersecret"
        "#,
        );
        let preflight = config
            .preflight_with_keys(&["tailscale_authkey".to_string()], None)
            .unwrap();
        assert_eq!(
            preflight.flat_vars().get("tailscale_authkey").unwrap(),
            "tskey-supersecret"
        );
    }

    // ── Host-scoped view (ADR-0058) ───────────────────────────────────────────

    const HOST_SCOPED: &str = r#"
        domain = "example.com"
        headscale_subdomain = "hs"

        [hosts.ruche]
        headscale_subdomain = ""

        [hosts.staging]
        domain = "staging.example.com"
        extra_key = "only-here"
    "#;

    #[test]
    fn test_host_override_wins_for_that_hosts_flat_vars() {
        let config = make_config(HOST_SCOPED);
        let flat = config.flatten_for_ansible(Some("staging")).unwrap();
        assert_eq!(flat.get("domain").unwrap(), "staging.example.com");
        assert_eq!(flat.get("extra_key").unwrap(), "only-here");
    }

    #[test]
    fn test_other_hosts_and_fleet_view_ignore_an_override() {
        let config = make_config(HOST_SCOPED);
        for host in [Some("auberge"), None] {
            let flat = config.flatten_for_ansible(host).unwrap();
            assert_eq!(flat.get("domain").unwrap(), "example.com", "{host:?}");
            assert_eq!(flat.get("headscale_subdomain").unwrap(), "hs", "{host:?}");
            assert!(!flat.contains_key("extra_key"), "{host:?}");
        }
    }

    /// The reserved table must never flatten wholesale: `flatten_toml` hoists
    /// nested leaves under their leaf names, which would leak one Host's
    /// overrides into every other Host's run.
    #[test]
    fn test_hosts_table_never_leaks_into_flat_vars() {
        let config = make_config(HOST_SCOPED);
        let flat = config.flatten_for_ansible(Some("ruche")).unwrap();
        assert!(!flat.contains_key("extra_key"));
        assert!(!flat.contains_key("hosts"));
    }

    /// A blank override must reach Ansible as an empty string, not vanish: the
    /// ADR-0051 gate expression reads defined-but-empty as "does not serve".
    #[test]
    fn test_blank_override_reaches_flat_vars_as_empty() {
        let config = make_config(HOST_SCOPED);
        let flat = config.flatten_for_ansible(Some("ruche")).unwrap();
        assert_eq!(flat.get("headscale_subdomain").unwrap(), "");
    }

    /// Naming a guarded role's tag against a Host that blanked its gate fails
    /// loudly (ADR-0045): the blank counts as missing for that Host only.
    #[test]
    fn test_blank_override_fails_preflight_for_that_host_only() {
        let config = make_config(HOST_SCOPED);
        let keys = vec!["headscale_subdomain".to_string()];
        let err = config
            .preflight_with_keys(&keys, Some("ruche"))
            .unwrap_err();
        assert!(err.to_string().contains("headscale_subdomain"), "{err}");
        assert!(config.preflight_with_keys(&keys, Some("auberge")).is_ok());
        assert!(config.preflight_with_keys(&keys, None).is_ok());
    }

    #[test]
    fn test_override_satisfies_a_key_missing_at_top_level() {
        let config = make_config(HOST_SCOPED);
        let keys = vec!["extra_key".to_string()];
        assert!(config.preflight_with_keys(&keys, Some("staging")).is_ok());
        let err = config
            .preflight_with_keys(&keys, Some("ruche"))
            .unwrap_err();
        assert!(err.to_string().contains("extra_key"), "{err}");
    }

    #[test]
    fn test_get_for_host_prefers_the_hosts_table() {
        let config = make_config(HOST_SCOPED);
        assert_eq!(
            config.get_for_host("domain", Some("staging")).unwrap(),
            "staging.example.com"
        );
        assert_eq!(
            config.get_for_host("domain", Some("ruche")).unwrap(),
            "example.com"
        );
        assert_eq!(
            config
                .get_for_host("headscale_subdomain", Some("ruche"))
                .unwrap(),
            ""
        );
        assert_eq!(config.get_for_host("domain", None).unwrap(), "example.com");
    }

    /// `set` writes flat top-level keys only; a dotted key would land as a
    /// literal name no run can read. Host tables are edited in the file.
    #[test]
    fn test_set_rejects_nested_keys() {
        let mut config = make_config("");
        let err = config
            .set("hosts.ruche.headscale_subdomain", "x")
            .unwrap_err();
        assert!(err.to_string().contains("nested"), "{err}");
    }
}
