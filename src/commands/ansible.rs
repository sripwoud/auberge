use crate::models::inventory::Host;
use crate::models::playbook::Playbook;
use crate::selector::select_item;
use crate::services::ansible_runner::{run_bootstrap, run_playbook};
use crate::services::inventory::{get_host, get_hosts, get_playbooks};
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AnsibleCommands {
    Run {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Playbook path")]
        playbook: Option<PathBuf>,
        #[arg(short = 'C', long, help = "Run in check mode (dry run)")]
        check: bool,
        #[arg(short, long, help = "Only run tasks with these tags")]
        tags: Option<Vec<String>>,
    },
    Check {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Playbook path")]
        playbook: Option<PathBuf>,
    },
    Bootstrap {
        host: String,
        #[arg(long, default_value = "22", help = "SSH port for initial connection")]
        port: u16,
    },
}

fn select_or_use_host(host_arg: Option<String>) -> Result<Host> {
    match host_arg {
        Some(name) => get_host(&name, None),
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
            .ok_or_else(|| eyre::eyre!("No host selected"))
        }
    }
}

fn select_or_use_playbook(playbook_arg: Option<PathBuf>) -> Result<Playbook> {
    match playbook_arg {
        Some(path) => Ok(Playbook::from_path(path)),
        None => {
            let playbooks = get_playbooks(None)?;
            select_item(
                &playbooks,
                |p: &Playbook| {
                    format!(
                        "{} ({})",
                        p.name,
                        p.path.file_name().unwrap_or_default().to_string_lossy()
                    )
                },
                "Select playbook",
            )?
            .ok_or_else(|| eyre::eyre!("No playbook selected"))
        }
    }
}

pub fn run_ansible_run(
    host: Option<String>,
    playbook: Option<PathBuf>,
    check: bool,
    tags: Option<Vec<String>>,
) -> Result<()> {
    let selected_host = select_or_use_host(host)?;
    let selected_playbook = select_or_use_playbook(playbook)?;

    eprintln!(
        "Running {} on {}...",
        selected_playbook.name, selected_host.name
    );

    let result = run_playbook(
        &selected_playbook.path,
        &selected_host.name,
        check,
        tags.as_deref(),
        None,
        true,
    )?;

    if result.success {
        eprintln!("✓ Playbook completed successfully");
        Ok(())
    } else {
        eyre::bail!("Playbook failed with exit code {}", result.exit_code)
    }
}

pub fn run_ansible_check(host: Option<String>, playbook: Option<PathBuf>) -> Result<()> {
    run_ansible_run(host, playbook, true, None)
}

pub fn run_ansible_bootstrap(host_name: String, port: u16) -> Result<()> {
    let host = get_host(&host_name, None)?;
    let bootstrap_playbook =
        crate::services::inventory::find_project_root().join("playbooks/bootstrap.yml");

    if !bootstrap_playbook.exists() {
        eyre::bail!(
            "Bootstrap playbook not found: {}",
            bootstrap_playbook.display()
        );
    }

    print!("Enter IP address for {}: ", host_name);
    io::stdout().flush()?;
    let mut host_ip = String::new();
    io::stdin()
        .read_line(&mut host_ip)
        .wrap_err("Failed to read IP address")?;
    let host_ip = host_ip.trim();

    eprintln!(
        "Bootstrapping {} ({}) as {}...",
        host_name, host_ip, host.vars.bootstrap_user
    );

    let result = run_bootstrap(
        &bootstrap_playbook,
        &host_name,
        host_ip,
        &host.vars.bootstrap_user,
        port,
    )?;

    if result.success {
        eprintln!("✓ Bootstrap completed successfully");
        Ok(())
    } else {
        eyre::bail!("Bootstrap failed with exit code {}", result.exit_code)
    }
}
