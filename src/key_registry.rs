use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;

/// A single entry in the Key Registry describing one configuration key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyEntry {
    /// Whether the value should be treated as a secret (redacted in output).
    pub secret: bool,
    /// Human-readable description of what the key configures.
    pub doc: String,
}

/// Raw deserialization wrapper for `keys.yml`.
#[derive(Debug, Deserialize)]
struct KeyRegistryFile {
    keys: HashMap<String, KeyEntry>,
}

/// Registry of all known configuration keys with their metadata.
#[derive(Debug, Clone)]
pub struct KeyRegistry {
    entries: HashMap<String, KeyEntry>,
}

impl KeyRegistry {
    /// Load the Key Registry from a `keys.yml` file at the given path.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read Key Registry from {}", path.display()))?;
        let file: KeyRegistryFile = serde_yaml::from_str(&contents)
            .wrap_err_with(|| format!("Failed to parse Key Registry from {}", path.display()))?;
        Ok(Self { entries: file.keys })
    }

    /// Returns the entry for a key by name, if it exists.
    pub fn get(&self, key: &str) -> Option<&KeyEntry> {
        self.entries.get(key)
    }

    /// Returns an iterator over all (name, entry) pairs in the registry.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &KeyEntry)> {
        self.entries.iter()
    }

    /// Returns the number of keys in the registry.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the registry contains no keys.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Render a TOML scaffold containing every key in the registry, sorted by name.
    pub fn scaffold(&self) -> String {
        self.render(self.entries.keys().map(String::as_str))
    }

    /// Render a TOML scaffold containing only keys whose name is in `selected`.
    /// Keys in `selected` that are absent from the registry are silently skipped.
    pub fn scaffold_filtered(&self, selected: &HashSet<String>) -> String {
        self.render(
            selected
                .iter()
                .filter(|k| self.entries.contains_key(*k))
                .map(String::as_str),
        )
    }

    fn render<'a>(&self, names: impl Iterator<Item = &'a str>) -> String {
        let mut sorted: Vec<&str> = names.collect();
        sorted.sort_unstable();
        let mut out = String::new();
        for name in sorted {
            let entry = &self.entries[name];
            let marker = if entry.secret { " (secret)" } else { "" };
            let _ = writeln!(out, "# {}{marker}", entry.doc);
            let _ = writeln!(out, "{name} = \"\"");
            let _ = writeln!(out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("keys.yml")
    }

    #[test]
    fn test_key_registry_loads_without_error() {
        let registry = KeyRegistry::load(&registry_path()).unwrap();
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_key_registry_contains_required_ansible_runner_keys() {
        let registry = KeyRegistry::load(&registry_path()).unwrap();
        for key in &[
            "admin_user_name",
            "ssh_port",
            "hostname",
            "domain",
            "tailscale_authkey",
            "cloudflare_dns_api_token",
            "hermes_llm_provider",
            "hermes_llm_api_key",
            "hermes_telegram_bot_token",
            "colporteur_subdomain",
            "tgtg_telegram_bot_token",
        ] {
            assert!(
                registry.get(key).is_some(),
                "Key Registry is missing key: {key}"
            );
        }
    }

    #[test]
    fn test_key_registry_contains_restic_keys() {
        let registry = KeyRegistry::load(&registry_path()).unwrap();
        assert!(registry.get("restic_repository").is_some());
        assert!(registry.get("restic_password").is_some());
    }

    #[test]
    fn test_key_registry_marks_secrets_correctly() {
        let registry = KeyRegistry::load(&registry_path()).unwrap();

        let secret_keys = [
            "admin_user_password",
            "cloudflare_dns_api_token",
            "hermes_llm_api_key",
            "hermes_telegram_bot_token",
            "restic_password",
            "tailscale_authkey",
            "tailscale_api_key",
        ];
        for key in &secret_keys {
            let entry = registry
                .get(key)
                .unwrap_or_else(|| panic!("missing key: {key}"));
            assert!(entry.secret, "Expected {key} to be marked as secret");
        }

        let public_keys = [
            "admin_user_name",
            "domain",
            "hostname",
            "ssh_port",
            "hermes_llm_provider",
        ];
        for key in &public_keys {
            let entry = registry
                .get(key)
                .unwrap_or_else(|| panic!("missing key: {key}"));
            assert!(!entry.secret, "Expected {key} to NOT be marked as secret");
        }
    }

    #[test]
    fn test_key_registry_all_entries_have_non_empty_doc() {
        let registry = KeyRegistry::load(&registry_path()).unwrap();
        for (name, entry) in registry.iter() {
            assert!(
                !entry.doc.trim().is_empty(),
                "Key '{name}' has an empty doc string"
            );
        }
    }

    #[test]
    fn test_key_registry_load_nonexistent_file_returns_error() {
        let result = KeyRegistry::load(Path::new("/nonexistent/keys.yml"));
        assert!(result.is_err());
    }

    fn fixture_registry() -> KeyRegistry {
        let mut entries = HashMap::new();
        entries.insert(
            "domain".into(),
            KeyEntry {
                secret: false,
                doc: "Primary domain name".into(),
            },
        );
        entries.insert(
            "tailscale_authkey".into(),
            KeyEntry {
                secret: true,
                doc: "Tailscale auth key".into(),
            },
        );
        entries.insert(
            "admin_user_name".into(),
            KeyEntry {
                secret: false,
                doc: "Admin username".into(),
            },
        );
        KeyRegistry { entries }
    }

    #[test]
    fn test_scaffold_emits_alphabetically_sorted_keys() {
        let scaffold = fixture_registry().scaffold();
        let admin_pos = scaffold.find("admin_user_name").unwrap();
        let domain_pos = scaffold.find("domain").unwrap();
        let tailscale_pos = scaffold.find("tailscale_authkey").unwrap();
        assert!(admin_pos < domain_pos);
        assert!(domain_pos < tailscale_pos);
    }

    #[test]
    fn test_scaffold_emits_doc_as_comment() {
        let scaffold = fixture_registry().scaffold();
        assert!(scaffold.contains("# Primary domain name\n"));
        assert!(scaffold.contains("# Admin username\n"));
    }

    #[test]
    fn test_scaffold_marks_secret_keys() {
        let scaffold = fixture_registry().scaffold();
        assert!(scaffold.contains("# Tailscale auth key (secret)\n"));
        assert!(!scaffold.contains("# Primary domain name (secret)"));
    }

    #[test]
    fn test_scaffold_emits_empty_string_placeholders() {
        let scaffold = fixture_registry().scaffold();
        assert!(scaffold.contains("admin_user_name = \"\""));
        assert!(scaffold.contains("domain = \"\""));
        assert!(scaffold.contains("tailscale_authkey = \"\""));
    }

    #[test]
    fn test_scaffold_output_parses_as_toml() {
        let scaffold = fixture_registry().scaffold();
        let parsed: toml::Table = toml::from_str(&scaffold).unwrap();
        assert!(parsed.contains_key("domain"));
        assert!(parsed.contains_key("admin_user_name"));
        assert!(parsed.contains_key("tailscale_authkey"));
    }

    #[test]
    fn test_scaffold_filtered_emits_only_selected_keys() {
        let mut selected = HashSet::new();
        selected.insert("domain".to_string());
        selected.insert("tailscale_authkey".to_string());
        let scaffold = fixture_registry().scaffold_filtered(&selected);
        assert!(scaffold.contains("domain = \"\""));
        assert!(scaffold.contains("tailscale_authkey = \"\""));
        assert!(!scaffold.contains("admin_user_name"));
    }

    #[test]
    fn test_scaffold_filtered_skips_unknown_keys() {
        let mut selected = HashSet::new();
        selected.insert("domain".to_string());
        selected.insert("does_not_exist".to_string());
        let scaffold = fixture_registry().scaffold_filtered(&selected);
        assert!(scaffold.contains("domain = \"\""));
        assert!(!scaffold.contains("does_not_exist"));
    }

    #[test]
    fn test_scaffold_filtered_empty_selection_emits_nothing() {
        let scaffold = fixture_registry().scaffold_filtered(&HashSet::new());
        assert!(scaffold.is_empty());
    }

    #[test]
    fn test_real_registry_scaffold_parses_as_toml() {
        let registry = KeyRegistry::load(&registry_path()).unwrap();
        let scaffold = registry.scaffold();
        let parsed: toml::Table = toml::from_str(&scaffold).unwrap();
        assert_eq!(parsed.len(), registry.len());
    }
}
