use crate::models::inventory::Host;
use crate::models::playbook::Playbook;
use crate::selector::select_item;
use crate::services::inventory::{get_hosts, get_playbooks};
use clap::Subcommand;
use eyre::Result;

#[derive(Subcommand)]
pub enum SelectCommands {
    #[command(alias = "h")]
    Host {
        #[arg(short, long, help = "Filter hosts by group")]
        group: Option<String>,
    },
    #[command(alias = "p")]
    Playbook,
}

pub fn run_select_host(group: Option<String>) -> Result<()> {
    let hosts = get_hosts(group.as_deref(), None)?;

    if hosts.is_empty() {
        eyre::bail!("No hosts found");
    }

    let selected = select_item(
        &hosts,
        |h: &Host| {
            format!(
                "{} ({}:{})",
                h.name, h.vars.ansible_host, h.vars.ansible_port
            )
        },
        "Select host",
    )?;

    match selected {
        Some(host) => {
            println!("{}", host.name);
            Ok(())
        }
        None => eyre::bail!("No host selected"),
    }
}

pub fn run_select_playbook() -> Result<()> {
    let playbooks = get_playbooks(None)?;

    let selected = select_item(
        &playbooks,
        |p: &Playbook| {
            format!(
                "{} ({})",
                p.name,
                p.path.file_name().unwrap_or_default().to_string_lossy()
            )
        },
        "Select playbook",
    )?;

    match selected {
        Some(playbook) => {
            println!("{}", playbook.path.display());
            Ok(())
        }
        None => eyre::bail!("No playbook selected"),
    }
}
