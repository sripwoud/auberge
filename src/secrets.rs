use eyre::{Context, Result};
use keyring::Entry;

const SERVICE_NAME: &str = "auberge";

pub struct SecretsManager;

impl SecretsManager {
    #[allow(dead_code)]
    pub fn get(key: &str) -> Result<String> {
        if let Ok(value) = std::env::var(key) {
            return Ok(value);
        }

        let entry = Entry::new(SERVICE_NAME, key)
            .wrap_err_with(|| format!("Failed to access keyring for key: {}", key))?;

        entry
            .get_password()
            .wrap_err_with(|| format!("Secret not found in keyring: {}", key))
    }

    #[allow(dead_code)]
    pub fn set(key: &str, value: &str) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, key)
            .wrap_err_with(|| format!("Failed to access keyring for key: {}", key))?;

        entry
            .set_password(value)
            .wrap_err_with(|| format!("Failed to store secret in keyring: {}", key))
    }

    #[allow(dead_code)]
    pub fn delete(key: &str) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, key)
            .wrap_err_with(|| format!("Failed to access keyring for key: {}", key))?;

        entry
            .delete_credential()
            .wrap_err_with(|| format!("Failed to delete secret from keyring: {}", key))
    }

    #[allow(dead_code)]
    pub fn required_secrets() -> Vec<&'static str> {
        vec![
            "ADMIN_USER_NAME",
            "ADMIN_USER_EMAIL",
            "PRIMARY_DOMAIN",
            "CLOUDFLARE_DNS_API_TOKEN",
            "RADICALE_PASSWORD",
            "WEBDAV_PASSWORD",
            "TAILSCALE_AUTHKEY",
            "SSH_PORT",
            "AUBERGE_HOST",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_secrets_list() {
        let secrets = SecretsManager::required_secrets();
        assert!(!secrets.is_empty());
        assert!(secrets.contains(&"ADMIN_USER_NAME"));
    }
}
