use crate::config::Config;
use crate::models::inventory::Host;
use crate::selector::select_item;
use crate::services::inventory::get_hosts;
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::path::PathBuf;
use std::process::Command;

#[derive(Subcommand)]
pub enum SyncCommands {
    Music {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Source music directory")]
        source: Option<PathBuf>,
        #[arg(short = 'n', long, help = "Dry run (don't actually sync)")]
        dry_run: bool,
    },
}

pub fn run_sync_music(
    host_arg: Option<String>,
    source: Option<PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let config = Config::load()?;
    let username = &config.user.username;

    let host = match host_arg {
        Some(name) => crate::services::inventory::get_host(&name, None)?,
        None => {
            let hosts = get_hosts(Some("selfhosted"), None)?;
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

    let music_source = source.unwrap_or_else(|| {
        dirs::home_dir()
            .map(|h| h.join("Music"))
            .unwrap_or_else(|| PathBuf::from("~/Music"))
    });

    if !music_source.exists() {
        eyre::bail!(
            "Music source directory not found: {}",
            music_source.display()
        );
    }

    let ssh_key = dirs::home_dir()
        .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
        .join(format!(".ssh/identities/{}_{}", username, host.name));

    if !ssh_key.exists() {
        eyre::bail!(
            "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}' first",
            ssh_key.display(),
            host.name,
            username
        );
    }

    let remote_path = "/srv/music/";
    let remote_user = username;

    eprintln!(
        "Syncing music to {}@{}:{}...",
        username, host.vars.ansible_host, remote_path
    );

    let mut cmd = Command::new("rsync");
    cmd.arg("-avzP")
        .arg("--delete")
        .arg("--exclude=.DS_Store")
        .arg("--exclude=*.tmp")
        .arg("-e")
        .arg(format!(
            "ssh -p {} -i {}",
            host.vars.ansible_port,
            ssh_key.display()
        ))
        .arg(format!("{}/", music_source.display()))
        .arg(format!(
            "{}@{}:{}",
            remote_user, host.vars.ansible_host, remote_path
        ));

    if dry_run {
        cmd.arg("--dry-run");
        eprintln!("(dry run mode)");
    }

    let status = cmd.status().wrap_err("Failed to execute rsync")?;

    if status.success() {
        eprintln!("✓ Music sync completed");
        Ok(())
    } else {
        eyre::bail!("rsync failed")
    }
}
