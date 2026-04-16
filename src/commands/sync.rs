use crate::hosts::HostManager;
use crate::models::inventory::Host;
use crate::output;
use crate::selector::select_item;
use crate::services::inventory::get_hosts;
use crate::ssh_session::SshSession;
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::path::PathBuf;
use std::process::Command;

#[derive(Subcommand)]
pub enum SyncCommands {
    #[command(alias = "m")]
    Music {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Source music directory")]
        source: Option<PathBuf>,
        #[arg(short = 'n', long, help = "Dry run (don't actually sync)")]
        dry_run: bool,
    },
    #[command(alias = "h", about = "Sync hermes config and restart service")]
    Hermes {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            help = "Config file path: source when pushing, destination when pulling [default: ~/.config/hermes/config.yaml]"
        )]
        source: Option<PathBuf>,
        #[arg(
            short = 'n',
            long,
            help = "Dry run (don't actually sync)",
            conflicts_with = "pull"
        )]
        dry_run: bool,
        #[arg(
            short = 'p',
            long,
            help = "Pull config from remote to local instead of pushing"
        )]
        pull: bool,
    },
}

pub fn run_sync_music(
    host_arg: Option<String>,
    source: Option<PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let ansible_user = "ansible";

    let host = match host_arg {
        Some(name) => crate::services::inventory::get_host(&name, None)?,
        None => {
            let hosts = get_hosts(Some("auberge"), None)?;
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
        .join(format!(".ssh/identities/{}_{}", ansible_user, host.name));

    if !ssh_key.exists() {
        eyre::bail!(
            "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}' first",
            ssh_key.display(),
            host.name,
            ansible_user
        );
    }

    let remote_path = "/srv/music/";

    output::info(&format!(
        "Syncing music to {}@{}:{}",
        ansible_user, host.vars.ansible_host, remote_path
    ));

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
            ansible_user, host.vars.ansible_host, remote_path
        ));

    if dry_run {
        cmd.arg("--dry-run");
        output::info("Dry run mode");
    }

    let result = output::run_piped("rsync", &mut cmd).wrap_err("Failed to execute rsync")?;
    if result.status.success() {
        output::clear_subprocess_lines(result.lines_written);
        output::success("Music sync completed");
        Ok(())
    } else {
        eyre::bail!("rsync failed")
    }
}

pub fn run_sync_hermes(
    host_arg: Option<String>,
    source: Option<PathBuf>,
    dry_run: bool,
    pull: bool,
) -> Result<()> {
    let xdg_host = match host_arg {
        Some(name) => HostManager::get_host(&name)?,
        None => {
            let hosts = HostManager::load_hosts()?;
            select_item(
                &hosts,
                |h: &crate::hosts::Host| format!("{} ({}:{})", h.name, h.address, h.port),
                "Select host",
            )?
            .ok_or_else(|| eyre::eyre!("No host selected"))?
        }
    };

    if pull {
        let local_dest = match source {
            Some(s) => s,
            None => dirs::home_dir()
                .map(|h| h.join(".config/hermes/config.yaml"))
                .ok_or_else(|| {
                    eyre::eyre!("Could not determine home directory for Hermes config")
                })?,
        };
        let ssh_key = xdg_host
            .ssh_key
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| eyre::eyre!("No SSH key configured for host '{}'", xdg_host.name))?;
        if !ssh_key.exists() {
            eyre::bail!("SSH key not found: {}", ssh_key.display());
        }
        if let Some(parent) = local_dest.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        let session = SshSession::new(&xdg_host, &ssh_key);
        output::info(&format!(
            "Pulling hermes config from remote to {}",
            local_dest.display()
        ));
        session.scp_from(".hermes/config.yaml", &local_dest)?;
        output::success("Hermes config pulled");
        return Ok(());
    }

    let config_source = match source {
        Some(s) => s,
        None => dirs::home_dir()
            .map(|h| h.join(".config/hermes/config.yaml"))
            .ok_or_else(|| eyre::eyre!("Could not determine home directory for Hermes config"))?,
    };

    if !config_source.exists() {
        eyre::bail!(
            "Hermes config not found: {}\nCreate it at ~/.config/hermes/config.yaml first",
            config_source.display()
        );
    }

    let ssh_key = xdg_host
        .ssh_key
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| eyre::eyre!("No SSH key configured for host '{}'", xdg_host.name))?;

    if !ssh_key.exists() {
        eyre::bail!("SSH key not found: {}", ssh_key.display());
    }

    let session = SshSession::new(&xdg_host, &ssh_key);
    let remote_dest = format!("{}@{}:.hermes/config.yaml", xdg_host.user, xdg_host.address);

    output::info("Preparing remote ~/.hermes directory...");
    let prepare = session
        .run("mkdir -p ~/.hermes")
        .wrap_err("Failed to prepare remote ~/.hermes directory")?;
    if !prepare.status.success() {
        eyre::bail!("Remote ~/.hermes directory is missing and could not be created");
    }

    output::info(&format!("Syncing hermes config to {}", remote_dest));

    let mut cmd = Command::new("rsync");
    cmd.arg("-az")
        .arg("-e")
        .arg(session.rsync_e_arg())
        .arg(&config_source)
        .arg(&remote_dest);

    if dry_run {
        cmd.arg("--dry-run");
        output::info("Dry run mode");
    }

    let result = output::run_piped("rsync", &mut cmd).wrap_err("Failed to execute rsync")?;
    if !result.status.success() {
        eyre::bail!("rsync failed");
    }
    output::clear_subprocess_lines(result.lines_written);
    output::success("Hermes config synced");

    if dry_run {
        return Ok(());
    }

    output::info("Restarting hermes-gateway...");
    let restart = session
        .run("XDG_RUNTIME_DIR=/run/user/$(id -u) systemctl --user restart hermes-gateway")
        .wrap_err("Failed to restart hermes-gateway")?;
    if !restart.status.success() {
        eyre::bail!("hermes-gateway restart failed");
    }
    output::success("hermes-gateway restarted");

    Ok(())
}
