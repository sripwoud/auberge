use crate::prompt::{Choice, select_item};
use crate::services::inventory::{Host, get_hosts, get_playbooks};
use clap::Subcommand;
use eyre::Result;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum SelectCommands {
    #[command(
        visible_alias = "h",
        about = "Print the name of an interactively selected host"
    )]
    Host {
        #[arg(short, long, help = "Filter hosts by group")]
        group: Option<String>,
    },
    #[command(
        visible_alias = "p",
        about = "Print the path of an interactively selected playbook"
    )]
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
        Choice::new("host"),
    )?;

    println!("{}", selected.name);
    Ok(())
}

pub fn run_select_playbook() -> Result<()> {
    let playbooks = get_playbooks(None)?;

    let selected = select_item(
        &playbooks,
        |p: &PathBuf| {
            let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
            let file = p.file_name().unwrap_or_default().to_string_lossy();
            format!("{} ({})", name, file)
        },
        Choice::new("playbook"),
    )?;

    println!("{}", selected.display());
    Ok(())
}
