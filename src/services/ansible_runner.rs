use crate::user_config::UserConfig;
use eyre::{Result, WrapErr};
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub struct AnsibleResult {
    pub success: bool,
    pub exit_code: i32,
}

pub struct InventoryHost {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub user: String,
}

fn write_extra_vars_file() -> Result<tempfile::NamedTempFile> {
    let config = UserConfig::load()?;
    let flat = config
        .flatten_for_ansible()
        .wrap_err("Failed to resolve config values")?;
    let yaml = serde_yaml::to_string(&flat).wrap_err("Failed to serialize config to YAML")?;
    let mut tmpfile = tempfile::NamedTempFile::new().wrap_err("Failed to create temp file")?;
    tmpfile
        .write_all(yaml.as_bytes())
        .wrap_err("Failed to write extra-vars file")?;
    Ok(tmpfile)
}

fn write_inventory_file(host: &InventoryHost) -> Result<tempfile::NamedTempFile> {
    use serde_yaml::{Mapping, Value};

    let mut host_vars = Mapping::new();
    host_vars.insert(
        Value::String("ansible_host".into()),
        Value::String(host.address.clone()),
    );
    host_vars.insert(
        Value::String("ansible_port".into()),
        Value::Number(host.port.into()),
    );

    let mut hosts = Mapping::new();
    hosts.insert(Value::String(host.name.clone()), Value::Mapping(host_vars));

    let mut vps = Mapping::new();
    vps.insert(Value::String("hosts".into()), Value::Mapping(hosts));

    let mut children = Mapping::new();
    children.insert(Value::String("vps".into()), Value::Mapping(vps));

    let mut all = Mapping::new();
    all.insert(Value::String("children".into()), Value::Mapping(children));

    let mut root = Mapping::new();
    root.insert(Value::String("all".into()), Value::Mapping(all));

    let yaml =
        serde_yaml::to_string(&Value::Mapping(root)).wrap_err("Failed to serialize inventory")?;

    let mut tmpfile = tempfile::NamedTempFile::new().wrap_err("Failed to create temp file")?;
    tmpfile
        .write_all(yaml.as_bytes())
        .wrap_err("Failed to write inventory file")?;
    Ok(tmpfile)
}

fn tag_required_keys(tag: &str) -> &[&'static str] {
    match tag {
        "colporteur" => &["colporteur_subdomain"],
        _ => &[],
    }
}

pub fn required_config_keys(playbook_name: &str, tags: Option<&[String]>) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Vec::new();

    match playbook_name {
        "bootstrap.yml" => {
            keys.extend(["admin_user_name", "ssh_port", "hostname"]);
        }
        "hardening.yml" => {}
        "infrastructure.yml" => {
            keys.extend(["admin_user_name", "domain", "tailscale_authkey"]);
        }
        "apps.yml" => {
            keys.extend(["admin_user_name", "domain", "cloudflare_dns_api_token"]);
        }
        "openclaw.yml" => {
            keys.extend([
                "admin_user_name",
                "domain",
                "openclaw_gateway_token",
                "openclaw_claude_ai_session_key",
            ]);
        }
        _ => {
            keys.extend(["admin_user_name", "domain"]);
        }
    }

    if playbook_name == "apps.yml"
        && let Some(tags) = tags
    {
        for tag in tags {
            for key in tag_required_keys(tag) {
                if !keys.contains(key) {
                    keys.push(key);
                }
            }
        }
    }

    keys
}

pub fn run_playbook(
    playbook: &Path,
    host: &InventoryHost,
    check: bool,
    tags: Option<&[String]>,
    skip_tags: Option<&[String]>,
    extra_vars: Option<&[(&str, &str)]>,
    ask_vault_pass: bool,
    ask_pass: bool,
) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    assets.ensure_collections()?;
    let ansible_dir = assets.ansible_dir().to_path_buf();
    let vars_file = write_extra_vars_file()?;
    let inventory_file = write_inventory_file(host)?;

    let mut cmd = Command::new("ansible-playbook");
    cmd.current_dir(&ansible_dir)
        .arg("-i")
        .arg("inventory.yml")
        .arg("-i")
        .arg(inventory_file.path())
        .arg(playbook.strip_prefix(&ansible_dir).unwrap_or(playbook))
        .arg("--limit")
        .arg(&host.name)
        .arg("--extra-vars")
        .arg(format!("@{}", vars_file.path().display()));

    let playbook_name = playbook.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_fresh_bootstrap = playbook_name == "bootstrap.yml";

    if check {
        cmd.arg("--check");
    }

    if ask_vault_pass {
        cmd.arg("--ask-vault-pass");
    }

    if is_fresh_bootstrap {
        cmd.arg("--ask-pass");
        cmd.arg("-e").arg(format!("ansible_port={}", host.port));
        cmd.arg("-e").arg(format!("ansible_user={}", host.user));
        cmd.arg("-e").arg(
            "ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'",
        );
    }

    if ask_pass && !is_fresh_bootstrap {
        cmd.arg("--ask-pass");
    }

    if let Some(tags) = tags {
        cmd.arg("--tags").arg(tags.join(","));
    }

    if let Some(skip_tags) = skip_tags {
        cmd.arg("--skip-tags").arg(skip_tags.join(","));
    }

    if let Some(vars) = extra_vars {
        for (key, value) in vars {
            cmd.arg("-e").arg(format!("{}={}", key, value));
        }
    }

    let status = cmd
        .status()
        .wrap_err("Failed to execute ansible-playbook")?;

    Ok(AnsibleResult {
        success: status.success(),
        exit_code: status.code().unwrap_or(-1),
    })
}

pub fn run_bootstrap(playbook: &Path, host: &InventoryHost) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    assets.ensure_collections()?;
    let ansible_dir = assets.ansible_dir().to_path_buf();
    let vars_file = write_extra_vars_file()?;
    let inventory_file = write_inventory_file(host)?;

    let status = Command::new("ansible-playbook")
        .current_dir(&ansible_dir)
        .arg("-i")
        .arg("inventory.yml")
        .arg("-i")
        .arg(inventory_file.path())
        .arg(playbook.strip_prefix(&ansible_dir).unwrap_or(playbook))
        .arg("--limit")
        .arg(&host.name)
        .arg("--extra-vars")
        .arg(format!("@{}", vars_file.path().display()))
        .arg("-e")
        .arg(format!("ansible_user={}", host.user))
        .arg("-e")
        .arg(format!("ansible_port={}", host.port))
        .arg("--ask-pass")
        .status()
        .wrap_err("Failed to execute ansible-playbook")?;

    Ok(AnsibleResult {
        success: status.success(),
        exit_code: status.code().unwrap_or(-1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_config_keys_bootstrap() {
        let keys = required_config_keys("bootstrap.yml", None);
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"ssh_port"));
        assert!(keys.contains(&"hostname"));
    }

    #[test]
    fn test_required_config_keys_infrastructure() {
        let keys = required_config_keys("infrastructure.yml", None);
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"domain"));
        assert!(keys.contains(&"tailscale_authkey"));
    }

    #[test]
    fn test_required_config_keys_apps() {
        let keys = required_config_keys("apps.yml", None);
        assert!(keys.contains(&"cloudflare_dns_api_token"));
        assert!(!keys.contains(&"colporteur_subdomain"));
    }

    #[test]
    fn test_required_config_keys_apps_with_colporteur_tag() {
        let tags = vec!["colporteur".to_string()];
        let keys = required_config_keys("apps.yml", Some(&tags));
        assert!(keys.contains(&"cloudflare_dns_api_token"));
        assert!(keys.contains(&"colporteur_subdomain"));
    }

    #[test]
    fn test_required_config_keys_apps_with_unrelated_tag() {
        let tags = vec!["paperless".to_string()];
        let keys = required_config_keys("apps.yml", Some(&tags));
        assert!(!keys.contains(&"colporteur_subdomain"));
    }

    #[test]
    fn test_required_config_keys_ignores_tags_for_non_apps_playbooks() {
        let tags = vec!["colporteur".to_string()];
        let keys = required_config_keys("infrastructure.yml", Some(&tags));
        assert!(!keys.contains(&"colporteur_subdomain"));
    }

    #[test]
    fn test_required_config_keys_hardening_is_empty() {
        let keys = required_config_keys("hardening.yml", None);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_required_config_keys_openclaw() {
        let keys = required_config_keys("openclaw.yml", None);
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"domain"));
        assert!(keys.contains(&"openclaw_gateway_token"));
        assert!(keys.contains(&"openclaw_claude_ai_session_key"));
    }

    #[test]
    fn test_required_config_keys_unknown_playbook_returns_defaults() {
        let keys = required_config_keys("custom.yml", None);
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"domain"));
    }

    #[test]
    fn test_write_inventory_file_generates_valid_yaml() {
        let host = InventoryHost {
            name: "testhost".to_string(),
            address: "198.51.100.1".to_string(),
            port: 59865,
            user: "root".to_string(),
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let host_entry = &parsed["all"]["children"]["vps"]["hosts"]["testhost"];
        assert_eq!(host_entry["ansible_host"].as_str().unwrap(), "198.51.100.1");
        assert_eq!(host_entry["ansible_port"].as_u64().unwrap(), 59865);
    }

    #[test]
    fn test_write_inventory_file_places_host_in_vps_group() {
        let host = InventoryHost {
            name: "myserver".to_string(),
            address: "203.0.113.42".to_string(),
            port: 22,
            user: "debian".to_string(),
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        assert!(parsed["all"]["children"]["vps"]["hosts"]["myserver"].is_mapping());
    }

    #[test]
    fn test_write_inventory_file_escapes_special_chars() {
        let host = InventoryHost {
            name: "host:with#special".to_string(),
            address: "198.51.100.1".to_string(),
            port: 22,
            user: "root".to_string(),
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let host_entry = &parsed["all"]["children"]["vps"]["hosts"]["host:with#special"];
        assert_eq!(host_entry["ansible_host"].as_str().unwrap(), "198.51.100.1");
    }
}
