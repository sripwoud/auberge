use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
}
