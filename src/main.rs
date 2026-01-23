mod commands;
mod config;
mod hosts;
mod models;
mod playbooks;
mod secrets;
mod selector;
mod services;

use clap::{Parser, Subcommand};
use commands::ansible::{
    AnsibleCommands, run_ansible_bootstrap, run_ansible_check, run_ansible_run,
};
use commands::backup::{
    BackupCommands, run_backup_create, run_backup_list, run_backup_restore, run_export_opml,
    run_import_opml,
};
use commands::dns::{
    DnsCommands, run_dns_list, run_dns_migrate, run_dns_set, run_dns_set_all, run_dns_status,
};
use commands::host::{
    AddHostArgs, HostCommands, run_host_add, run_host_edit, run_host_list, run_host_remove,
    run_host_show,
};
use commands::select::{SelectCommands, run_select_host, run_select_playbook};
use commands::ssh::{SshCommands, run_ssh_add_key, run_ssh_keygen};
use commands::sync::{SyncCommands, run_sync_music};
use eyre::Result;

#[derive(Parser)]
#[command(name = "auberge")]
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
    #[command(
        subcommand,
        alias = "se",
        about = "Select hosts or playbooks interactively"
    )]
    Select(SelectCommands),
    #[command(subcommand, alias = "a", about = "Run ansible playbooks")]
    Ansible(AnsibleCommands),
    #[command(subcommand, alias = "b", about = "Backup and restore application data")]
    Backup(BackupCommands),
    #[command(subcommand, alias = "h", about = "Manage VPS hosts")]
    Host(HostCommands),
    #[command(subcommand, alias = "ss", about = "SSH key management")]
    Ssh(SshCommands),
    #[command(subcommand, alias = "sy", about = "Sync files to remote hosts")]
    Sync(SyncCommands),
    #[command(subcommand, alias = "d", about = "DNS management via Namecheap")]
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
        Commands::Host(cmd) => match cmd {
            HostCommands::Add {
                name,
                address,
                user,
                port,
                ssh_key,
                tags,
                description,
                no_input,
            } => run_host_add(AddHostArgs {
                name,
                address,
                user,
                port,
                ssh_key,
                tags,
                description,
                no_input,
            }),
            HostCommands::List { tags, output } => run_host_list(tags, output),
            HostCommands::Remove { name, yes } => run_host_remove(name, yes),
            HostCommands::Show { name, output } => run_host_show(name, output),
            HostCommands::Edit { name } => run_host_edit(name),
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
        Commands::Backup(cmd) => match cmd {
            BackupCommands::Create {
                host,
                apps,
                dest,
                ssh_key,
                include_music,
                dry_run,
            } => run_backup_create(host, apps, dest, ssh_key, include_music, dry_run),
            BackupCommands::List { host, app, format } => run_backup_list(host, app, format),
            BackupCommands::Restore {
                backup_id,
                host,
                apps,
                ssh_key,
                dry_run,
                yes,
            } => run_backup_restore(backup_id, host, apps, ssh_key, dry_run, yes),
            BackupCommands::ExportOpml {
                host,
                output,
                ssh_key,
                user,
            } => run_export_opml(host, output, ssh_key, user),
            BackupCommands::ImportOpml {
                host,
                input,
                ssh_key,
                user,
            } => run_import_opml(host, input, ssh_key, user),
        },
        Commands::Ssh(cmd) => match cmd {
            SshCommands::Keygen { host, user, force } => run_ssh_keygen(host, user, force),
            SshCommands::AddKey {
                host,
                connect_with,
                authorize,
                user,
                yes,
            } => run_ssh_add_key(host, connect_with, authorize, user, yes),
        },
        Commands::Sync(cmd) => match cmd {
            SyncCommands::Music {
                host,
                source,
                dry_run,
            } => run_sync_music(host, source, dry_run),
        },
        Commands::Dns(cmd) => match cmd {
            DnsCommands::List {
                subdomain,
                production,
            } => run_dns_list(subdomain, production).await,
            DnsCommands::Status { production } => run_dns_status(production).await,
            DnsCommands::Set {
                subdomain,
                ip,
                production,
            } => run_dns_set(subdomain, ip, production).await,
            DnsCommands::Migrate {
                ip,
                dry_run,
                production,
            } => run_dns_migrate(ip, dry_run, production).await,
            DnsCommands::SetAll {
                host,
                ip,
                dry_run,
                yes,
                strict,
                subdomains,
                skip,
                output,
                continue_on_error,
                production,
            } => {
                run_dns_set_all(
                    host,
                    ip,
                    dry_run,
                    yes,
                    strict,
                    subdomains,
                    skip,
                    output,
                    continue_on_error,
                    production,
                )
                .await
            }
        },
    }
}
