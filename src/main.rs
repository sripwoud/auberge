mod commands;
mod models;
mod selector;
mod services;

use clap::{Parser, Subcommand};
use commands::ansible::{
    AnsibleCommands, run_ansible_bootstrap, run_ansible_check, run_ansible_run,
};
use commands::dns::{DnsCommands, run_dns_list, run_dns_migrate, run_dns_set, run_dns_status};
use commands::select::{SelectCommands, run_select_host, run_select_playbook};
use commands::ssh::{SshCommands, run_ssh_keygen};
use commands::sync::{SyncCommands, run_sync_music};
use eyre::Result;

#[derive(Parser)]
#[command(name = "selfhost")]
#[command(about = "CLI for selfhost infrastructure management")]
#[command(version)]
struct Cli {
    #[arg(short, long, global = true, help = "Enable verbose output")]
    verbose: bool,
    #[arg(short, long, global = true, help = "Suppress non-essential output")]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand, about = "Select hosts or playbooks interactively")]
    Select(SelectCommands),
    #[command(subcommand, about = "Run ansible playbooks")]
    Ansible(AnsibleCommands),
    #[command(subcommand, about = "SSH key management")]
    Ssh(SshCommands),
    #[command(subcommand, about = "Sync files to remote hosts")]
    Sync(SyncCommands),
    #[command(subcommand, about = "DNS management via Namecheap")]
    Dns(DnsCommands),
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Select(cmd) => match cmd {
            SelectCommands::Host { group } => run_select_host(group),
            SelectCommands::Playbook => run_select_playbook(),
        },
        Commands::Ansible(cmd) => match cmd {
            AnsibleCommands::Run {
                host,
                playbook,
                check,
                tags,
            } => run_ansible_run(host, playbook, check, tags),
            AnsibleCommands::Check { host, playbook } => run_ansible_check(host, playbook),
            AnsibleCommands::Bootstrap { host, port } => run_ansible_bootstrap(host, port),
        },
        Commands::Ssh(cmd) => match cmd {
            SshCommands::Keygen { host, user, force } => run_ssh_keygen(host, user, force),
        },
        Commands::Sync(cmd) => match cmd {
            SyncCommands::Music {
                host,
                source,
                dry_run,
            } => run_sync_music(host, source, dry_run),
        },
        Commands::Dns(cmd) => match cmd {
            DnsCommands::List { subdomain } => run_dns_list(subdomain).await,
            DnsCommands::Status => run_dns_status().await,
            DnsCommands::Set { subdomain, ip } => run_dns_set(subdomain, ip).await,
            DnsCommands::Migrate { ip, dry_run } => run_dns_migrate(ip, dry_run).await,
        },
    }
}
