use crate::services::inventory::find_project_root;
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
    let flat = config.flatten_for_ansible();
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

pub fn required_config_keys(playbook_name: &str) -> Vec<&'static str> {
    let mut keys: Vec<&str> = Vec::new();

    match playbook_name {
        "bootstrap.yml" => {
            keys.extend(["admin_user_name", "ssh_port"]);
        }
        "hardening.yml" => {
            keys.extend(["admin_user_name", "ssh_port"]);
        }
        "infrastructure.yml" => {
            keys.extend([
                "admin_user_name",
                "domain",
                "primary_domain",
                "tailscale_authkey",
            ]);
        }
        "apps.yml" => {
            keys.extend([
                "admin_user_name",
                "domain",
                "primary_domain",
                "cloudflare_dns_api_token",
                "zone_id",
            ]);
        }
        "auberge.yml" => {
            keys.extend([
                "admin_user_name",
                "domain",
                "primary_domain",
                "ssh_port",
                "cloudflare_dns_api_token",
                "tailscale_authkey",
                "zone_id",
            ]);
        }
        _ => {
            keys.extend(["admin_user_name", "domain", "primary_domain"]);
        }
    }

    keys
}

pub fn run_playbook(
    playbook: &Path,
    host: &InventoryHost,
    check: bool,
    tags: Option<&[String]>,
    extra_vars: Option<&[(&str, &str)]>,
    ask_vault_pass: bool,
    ask_pass: bool,
) -> Result<AnsibleResult> {
    let project_root = find_project_root();
    let ansible_dir = project_root.join("ansible");
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
    let project_root = find_project_root();
    let ansible_dir = project_root.join("ansible");
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
        let keys = required_config_keys("bootstrap.yml");
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"ssh_port"));
    }

    #[test]
    fn test_required_config_keys_infrastructure() {
        let keys = required_config_keys("infrastructure.yml");
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"domain"));
        assert!(keys.contains(&"primary_domain"));
        assert!(keys.contains(&"tailscale_authkey"));
    }

    #[test]
    fn test_required_config_keys_apps() {
        let keys = required_config_keys("apps.yml");
        assert!(keys.contains(&"cloudflare_dns_api_token"));
        assert!(keys.contains(&"zone_id"));
    }

    #[test]
    fn test_required_config_keys_auberge_is_superset() {
        let auberge = required_config_keys("auberge.yml");
        let bootstrap = required_config_keys("bootstrap.yml");
        let infra = required_config_keys("infrastructure.yml");
        let apps = required_config_keys("apps.yml");
        for key in bootstrap.iter().chain(infra.iter()).chain(apps.iter()) {
            assert!(auberge.contains(key), "auberge.yml missing key: {}", key);
        }
    }

    #[test]
    fn test_required_config_keys_unknown_playbook_returns_defaults() {
        let keys = required_config_keys("custom.yml");
        assert!(keys.contains(&"admin_user_name"));
        assert!(keys.contains(&"domain"));
        assert!(keys.contains(&"primary_domain"));
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
