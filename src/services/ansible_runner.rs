use crate::config::Preflight;
use crate::output;
use crate::services::progress::Progress;
use eyre::{Result, WrapErr};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn parse_ansible_task(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("TASK [")?;
    let end = rest.find(']')?;
    Some(rest[..end].to_string())
}

fn format_ansible_task(task: &str) -> String {
    if let Some((role, name)) = task.split_once(" : ") {
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            format!("{DIM}{}:{RESET} {}", role, name)
        } else {
            format!("{}: {}", role, name)
        }
    } else {
        task.to_string()
    }
}

pub struct AnsibleResult {
    pub success: bool,
    pub exit_code: i32,
    pub last_output: String,
}

pub struct InventoryHost {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub user: String,
}

fn write_extra_vars_file(flat_vars: &HashMap<String, String>) -> Result<tempfile::NamedTempFile> {
    let yaml = serde_yaml::to_string(flat_vars).wrap_err("Failed to serialize config to YAML")?;
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

pub fn run_playbook(
    preflight: &Preflight,
    playbook: &Path,
    host: &InventoryHost,
    check: bool,
    tags: Option<&[String]>,
    skip_tags: Option<&[String]>,
    extra_vars: Option<&[(&str, &str)]>,
    ask_vault_pass: bool,
    ask_pass: bool,
    progress: &mut dyn Progress,
) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    assets.ensure_collections()?;
    let ansible_dir = assets.ansible_dir().to_path_buf();
    let vars_file = write_extra_vars_file(preflight.flat_vars())?;
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

    let needs_tty = ask_vault_pass || ask_pass || is_fresh_bootstrap;
    if needs_tty {
        let status = cmd
            .status()
            .wrap_err("Failed to execute ansible-playbook")?;
        return Ok(AnsibleResult {
            success: status.success(),
            exit_code: status.code().unwrap_or(-1),
            last_output: String::new(),
        });
    }

    let playbook_label = playbook
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("ansible");
    progress.task_started(&format!("Running {}...", playbook_label));
    let result = output::stream_command_stdout("ansible", &mut cmd, |line| {
        if let Some(task) = parse_ansible_task(line) {
            progress.task_started(&format!("Running: {}", format_ansible_task(&task)));
        }
    })
    .wrap_err("Failed to execute ansible-playbook")?;
    progress.task_done();

    Ok(AnsibleResult {
        success: result.status.success(),
        exit_code: result.status.code().unwrap_or(-1),
        last_output: result.last_stderr,
    })
}

pub fn run_bootstrap(
    preflight: &Preflight,
    playbook: &Path,
    host: &InventoryHost,
) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    assets.ensure_collections()?;
    let ansible_dir = assets.ansible_dir().to_path_buf();
    let vars_file = write_extra_vars_file(preflight.flat_vars())?;
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
        last_output: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_ansible_task_extracts_name() {
        assert_eq!(
            parse_ansible_task("TASK [Install nginx] ***************************"),
            Some("Install nginx".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_with_role_prefix() {
        assert_eq!(
            parse_ansible_task("TASK [role : subtask name] ****"),
            Some("role : subtask name".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_gathering_facts() {
        assert_eq!(
            parse_ansible_task("TASK [Gathering Facts] *****"),
            Some("Gathering Facts".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_strips_leading_whitespace() {
        assert_eq!(
            parse_ansible_task("  TASK [Install nginx] ****"),
            Some("Install nginx".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_play_line_returns_none() {
        assert!(parse_ansible_task("PLAY [all] ****").is_none());
    }

    #[test]
    fn parse_ansible_task_ok_line_returns_none() {
        assert!(parse_ansible_task("ok: [hostname]").is_none());
    }

    #[test]
    fn parse_ansible_task_empty_returns_none() {
        assert!(parse_ansible_task("").is_none());
    }

    #[test]
    fn format_ansible_task_dims_role_prefix() {
        let formatted = format_ansible_task("nginx : Install package");
        assert!(formatted.contains("nginx:"));
        assert!(formatted.contains("Install package"));
    }

    #[test]
    fn format_ansible_task_no_role_returns_unchanged() {
        let formatted = format_ansible_task("Gathering Facts");
        assert_eq!(formatted, "Gathering Facts");
    }

    #[test]
    fn format_ansible_task_nested_role_splits_on_first_separator() {
        let formatted = format_ansible_task("role : sub : detail");
        assert!(formatted.contains("role:"));
        assert!(formatted.contains("sub : detail"));
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
