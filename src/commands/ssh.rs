use crate::models::inventory::Host;
use crate::selector::select_item;
use crate::services::inventory::get_hosts;
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::process::Command;

#[derive(Subcommand)]
pub enum SshCommands {
    Keygen {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            default_value = "ansible",
            help = "User (ansible or your configured username)"
        )]
        user: String,
        #[arg(short, long, help = "Force overwrite existing key")]
        force: bool,
    },
}

pub fn run_ssh_keygen(host_arg: Option<String>, user: String, force: bool) -> Result<()> {
    let host = match host_arg {
        Some(name) => crate::services::inventory::get_host(&name, None)?,
        None => {
            let hosts = get_hosts(None, None)?;
            select_item(
                &hosts,
                |h: &Host| {
                    format!(
                        "{} ({}:{})",
                        h.name, h.vars.ansible_host, h.vars.ansible_port
                    )
                },
                "Select host",
            )?
            .ok_or_else(|| eyre::eyre!("No host selected"))?
        }
    };

    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
        .join(".ssh/identities");

    std::fs::create_dir_all(&ssh_dir).wrap_err("Failed to create SSH identities directory")?;

    let key_path = ssh_dir.join(format!("{}_{}", user, host.name));

    if key_path.exists() && !force {
        eprintln!("✓ Key already exists: {}", key_path.display());
        return Ok(());
    }

    eprintln!("Generating SSH key for {}@{}...", user, host.name);

    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-t")
        .arg("ed25519")
        .arg("-f")
        .arg(&key_path)
        .arg("-C")
        .arg(format!("{}@{}", user, host.name))
        .arg("-N")
        .arg("");

    if force {
        cmd.arg("-y");
    }

    let status = cmd.status().wrap_err("Failed to execute ssh-keygen")?;

    if status.success() {
        eprintln!("✓ Generated key: {}", key_path.display());
        eprintln!("  Public key: {}.pub", key_path.display());
        Ok(())
    } else {
        eyre::bail!("ssh-keygen failed")
    }
}
