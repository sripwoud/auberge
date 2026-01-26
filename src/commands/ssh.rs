use crate::models::inventory::Host;
use crate::output;
use crate::selector::select_item;
use crate::services::inventory::get_hosts;
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::process::Command;

#[derive(Subcommand)]
pub enum SshCommands {
    #[command(alias = "k")]
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
    #[command(about = "Add/authorize SSH public key on remote host")]
    AddKey {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short = 'c', long, help = "SSH private key to connect with")]
        connect_with: Option<std::path::PathBuf>,
        #[arg(short = 'a', long, help = "Public key file to authorize on remote")]
        authorize: Option<std::path::PathBuf>,
        #[arg(
            short,
            long,
            default_value = "ansible",
            help = "Remote user to authorize key for"
        )]
        user: String,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
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
        output::success(&format!("Key already exists: {}", key_path.display()));
        return Ok(());
    }

    output::info(&format!("Generating SSH key for {}@{}", user, host.name));

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
        output::success(&format!("Generated key: {}", key_path.display()));
        output::info(&format!("Public key: {}.pub", key_path.display()));
        Ok(())
    } else {
        eyre::bail!("ssh-keygen failed")
    }
}

pub fn run_ssh_add_key(
    host_arg: Option<String>,
    connect_with: Option<std::path::PathBuf>,
    authorize: Option<std::path::PathBuf>,
    user: String,
    yes: bool,
) -> Result<()> {
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

    let home_dir =
        dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;

    let connect_key = match connect_with {
        Some(path) => path,
        None => {
            let default_key = home_dir
                .join(".ssh/identities")
                .join(format!("{}_{}", user, host.name));

            if default_key.exists() {
                output::info(&format!(
                    "Using default connection key: {}",
                    default_key.display()
                ));
                default_key
            } else {
                let available_keys = scan_private_keys(&home_dir)?;
                if available_keys.is_empty() {
                    eyre::bail!(
                        "No SSH private keys found. Generate one with 'auberge ssh keygen'"
                    );
                }

                select_item(
                    &available_keys,
                    |path| path.display().to_string(),
                    "Select SSH key to connect with",
                )?
                .ok_or_else(|| eyre::eyre!("No key selected"))?
            }
        }
    };

    if !connect_key.exists() {
        eyre::bail!("Connection key not found: {}", connect_key.display());
    }

    let pubkey_to_authorize = match authorize {
        Some(path) => path,
        None => {
            let available_pubkeys = scan_public_keys(&home_dir)?;
            if available_pubkeys.is_empty() {
                eyre::bail!("No public keys found in ~/.ssh/");
            }

            select_item(
                &available_pubkeys,
                |path| path.display().to_string(),
                "Select public key to authorize on remote",
            )?
            .ok_or_else(|| eyre::eyre!("No key selected"))?
        }
    };

    if !pubkey_to_authorize.exists() {
        eyre::bail!("Public key not found: {}", pubkey_to_authorize.display());
    }

    let pubkey_content = std::fs::read_to_string(&pubkey_to_authorize).wrap_err_with(|| {
        format!(
            "Failed to read public key: {}",
            pubkey_to_authorize.display()
        )
    })?;

    output::info("Add SSH Key");
    output::info(&format!(
        "Host: {} ({}:{})",
        host.name, host.vars.ansible_host, host.vars.ansible_port
    ));
    output::info(&format!("Remote user: {}", user));
    output::info(&format!("Connection key: {}", connect_key.display()));
    output::info(&format!(
        "Key to authorize: {}",
        pubkey_to_authorize.display()
    ));
    output::info(&format!("Public key preview: {}", pubkey_content.trim()));

    if !yes
        && !dialoguer::Confirm::new()
            .with_prompt("Authorize this key on the remote host?")
            .default(false)
            .interact()?
    {
        println!("Cancelled");
        return Ok(());
    }

    output::info("Adding key to remote host");

    let ssh_cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && echo 'Key added successfully'",
        pubkey_content.trim()
    );

    let status = Command::new("ssh")
        .arg("-i")
        .arg(&connect_key)
        .arg("-p")
        .arg(host.vars.ansible_port.to_string())
        .arg(format!("{}@{}", user, host.vars.ansible_host))
        .arg(ssh_cmd)
        .status()
        .wrap_err("Failed to execute SSH command")?;

    if !status.success() {
        eyre::bail!("Failed to add key to remote host");
    }

    output::success(&format!(
        "Key authorized successfully on {}@{}",
        user, host.name
    ));
    Ok(())
}

fn scan_private_keys(home_dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut keys = Vec::new();

    let identities_dir = home_dir.join(".ssh/identities");
    if identities_dir.is_dir() {
        for entry in std::fs::read_dir(&identities_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_none_or(|ext| ext != "pub") {
                keys.push(path);
            }
        }
    }

    let ssh_dir = home_dir.join(".ssh");
    if ssh_dir.is_dir() {
        for entry in std::fs::read_dir(&ssh_dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if path.is_file()
                && path.extension().is_none_or(|ext| ext != "pub")
                && (file_name.starts_with("id_") || file_name == "identity")
                && file_name != "known_hosts"
                && file_name != "authorized_keys"
                && file_name != "config"
            {
                keys.push(path);
            }
        }
    }

    keys.sort();
    keys.dedup();
    Ok(keys)
}

fn scan_public_keys(home_dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut keys = Vec::new();

    let identities_dir = home_dir.join(".ssh/identities");
    if identities_dir.is_dir() {
        for entry in std::fs::read_dir(&identities_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "pub") {
                keys.push(path);
            }
        }
    }

    let ssh_dir = home_dir.join(".ssh");
    if ssh_dir.is_dir() {
        for entry in std::fs::read_dir(&ssh_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "pub") {
                keys.push(path);
            }
        }
    }

    keys.sort();
    keys.dedup();
    Ok(keys)
}
