use crate::services::inventory::find_project_root;
use eyre::{Result, WrapErr};
use std::path::Path;
use std::process::Command;

pub struct AnsibleResult {
    pub success: bool,
    pub exit_code: i32,
}

pub fn run_playbook(
    playbook: &Path,
    host: &str,
    check: bool,
    tags: Option<&[String]>,
    extra_vars: Option<&[(&str, &str)]>,
    ask_vault_pass: bool,
    ask_pass: bool,
) -> Result<AnsibleResult> {
    let project_root = find_project_root();
    let ansible_dir = project_root.join("ansible");

    let mut cmd = Command::new("ansible-playbook");
    cmd.current_dir(&ansible_dir)
        .arg("-i")
        .arg("inventory.yml")
        .arg(playbook.strip_prefix(&ansible_dir).unwrap_or(playbook))
        .arg("--limit")
        .arg(host);

    // Detect if playbook is fresh bootstrap (standalone bootstrap.yml only)
    let playbook_name = playbook.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_fresh_bootstrap = playbook_name == "bootstrap.yml";

    if check {
        cmd.arg("--check");
    }

    if ask_vault_pass {
        cmd.arg("--ask-vault-pass");
    }

    // For fresh bootstrap only: override to port 22 with password auth
    // auberge.yml (full deployment) uses inventory settings (SSH_PORT + keys)
    if is_fresh_bootstrap {
        cmd.arg("--ask-pass");
        // Override to port 22 and disable strict checking for initial bootstrap connection
        // After bootstrap, inventory's SSH_PORT will be used with key-based auth
        cmd.arg("-e").arg("ansible_port=22");
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

pub fn run_bootstrap(
    playbook: &Path,
    host: &str,
    host_ip: &str,
    bootstrap_user: &str,
    port: u16,
) -> Result<AnsibleResult> {
    let project_root = find_project_root();
    let ansible_dir = project_root.join("ansible");

    let status = Command::new("ansible-playbook")
        .current_dir(&ansible_dir)
        .arg("-i")
        .arg("inventory.yml")
        .arg(playbook.strip_prefix(&ansible_dir).unwrap_or(playbook))
        .arg("--limit")
        .arg(host)
        .arg("-e")
        .arg(format!("ansible_port={}", port))
        .arg("-e")
        .arg(format!("ansible_user={}", bootstrap_user))
        .arg("-e")
        .arg(format!("ansible_host={}", host_ip))
        .arg("--ask-pass")
        .status()
        .wrap_err("Failed to execute ansible-playbook")?;

    Ok(AnsibleResult {
        success: status.success(),
        exit_code: status.code().unwrap_or(-1),
    })
}
