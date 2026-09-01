//! The `auberge` binary: the clap tree and the dispatch `match`, and nothing
//! else. Every module it dispatches into lives in `lib.rs`, so the integration
//! fences can import the same vocabulary the commands run on (#667).

use auberge::commands::ansible::{AnsibleCommands, run_ansible_bootstrap, run_ansible_run};
use auberge::commands::backup::{
    BackupCommands, RestoreOptions, VerifyOptions, create_parameters, run_backup_create,
    run_backup_list, run_backup_prune, run_backup_push, run_backup_restore, run_backup_sync,
    run_backup_verify,
};
use auberge::commands::bichon::{BichonCommands, run_bichon_command};
use auberge::commands::config_cmd::{
    ConfigCommands, run_config_edit, run_config_get, run_config_init, run_config_list,
    run_config_path, run_config_remove, run_config_set,
};
use auberge::commands::deploy::{DeployCmd, run_deploy};
use auberge::commands::dns::{
    DnsCommands, SetAllOptions, run_dns_delete, run_dns_list, run_dns_migrate, run_dns_set,
    run_dns_set_all, run_dns_status,
};
use auberge::commands::github::{GithubCommands, run_github_invite, run_github_verify};
use auberge::commands::headscale::{
    HeadscaleCommands, run_headscale_add_key, run_headscale_add_user, run_headscale_list_nodes,
    run_headscale_list_users, run_headscale_register, run_headscale_remove_user,
    run_headscale_tag_node,
};
use auberge::commands::host::{
    AddHostArgs, HostCommands, run_host_add, run_host_detect_tailscale_ip, run_host_edit,
    run_host_list, run_host_remove, run_host_rename, run_host_show,
};
use auberge::commands::opml::{OpmlCommands, run_export_opml, run_import_opml};
use auberge::commands::select::{SelectCommands, run_select_host, run_select_playbook};
use auberge::commands::ssh::{SshCommands, run_ssh_add_key, run_ssh_keygen};
use auberge::commands::sync::{SyncCommands, run_sync_hermes, run_sync_music};
use auberge::commands::versions::{VersionsCmd, run_versions};
use auberge::services::route::{self, Via};
use auberge::{output, signal};
use clap::{CommandFactory, Parser, Subcommand};
use eyre::Result;

#[derive(Parser)]
#[command(name = "auberge")]
#[command(about = "CLI for selfhost infrastructure management")]
#[command(version)]
struct Cli {
    #[arg(short, long, global = true, help = "Enable verbose output")]
    verbose: bool,
    #[arg(
        short,
        long,
        global = true,
        help = "Suppress non-essential output",
        conflicts_with = "verbose"
    )]
    quiet: bool,
    #[arg(
        long,
        global = true,
        help = "Disable colored output (also honored via NO_COLOR env var)"
    )]
    no_color: bool,
    #[arg(
        long,
        global = true,
        value_enum,
        // Not `ROUTE`: CONTEXT.md's **Route** is the resolved
        // address/port/user/key/alias tuple, and **Via**'s _Avoid_ list names
        // `--route`. Spelling the two values is also what the ADR and the docs
        // say.
        value_name = "public|tailnet",
        help = "Reach hosts over their public address or their tailnet address, overriding \
                each host's prefer_tailnet. `--via public` is the recovery route when the \
                tailnet is down"
    )]
    via: Option<Via>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(visible_alias = "dp", about = "Deploy apps to a host")]
    Deploy(DeployCmd),
    #[command(
        subcommand,
        visible_alias = "se",
        about = "Select hosts or playbooks interactively"
    )]
    Select(SelectCommands),
    #[command(subcommand, visible_alias = "a", about = "Run ansible playbooks")]
    Ansible(AnsibleCommands),
    #[command(
        subcommand,
        visible_alias = "b",
        about = "Backup and restore application data"
    )]
    Backup(BackupCommands),
    #[command(
        subcommand,
        visible_alias = "hs",
        about = "Manage Headscale VPN users and nodes"
    )]
    Headscale(HeadscaleCommands),
    #[command(subcommand, visible_alias = "h", about = "Manage VPS hosts")]
    Host(HostCommands),
    #[command(subcommand, visible_alias = "ss", about = "SSH key management")]
    Ssh(SshCommands),
    #[command(subcommand, visible_alias = "sy", about = "Sync files to remote hosts")]
    Sync(SyncCommands),
    #[command(
        subcommand,
        visible_alias = "d",
        about = "DNS management via Cloudflare"
    )]
    Dns(DnsCommands),
    #[command(subcommand, visible_alias = "c", about = "Manage user configuration")]
    Config(ConfigCommands),
    #[command(subcommand, about = "Manage Bichon email archive behavior")]
    Bichon(BichonCommands),
    #[command(subcommand, about = "Provision the GitHub machine user (ADR-0054)")]
    Github(GithubCommands),
    #[command(
        visible_alias = "v",
        about = "Report declared App and Tool Versions and upstream drift"
    )]
    Versions(VersionsCmd),
    #[command(about = "Generate shell completion script")]
    Completions { shell: clap_complete::Shell },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    output::set_verbose(cli.verbose);
    output::set_quiet(cli.quiet);
    output::set_no_color(cli.no_color);
    route::set_override(cli.via);

    let outcome = match cli.command {
        Commands::Deploy(cmd) => signal::with_ctrlc(|| run_deploy(cmd)),
        Commands::Select(cmd) => match cmd {
            SelectCommands::Host { group } => run_select_host(group),
            SelectCommands::Playbook => run_select_playbook(),
        },
        Commands::Headscale(cmd) => match cmd {
            HeadscaleCommands::AddUser {
                name,
                expiration,
                tags,
                host,
            } => run_headscale_add_user(name, expiration, tags, host),
            HeadscaleCommands::AddKey {
                user,
                expiration,
                tags,
                host,
            } => run_headscale_add_key(user, expiration, tags, host),
            HeadscaleCommands::Register { auth, user, host } => {
                run_headscale_register(auth, user, host)
            }
            HeadscaleCommands::TagNode { name, tags, host } => {
                run_headscale_tag_node(name, tags, host)
            }
            HeadscaleCommands::ListUsers { output, host } => run_headscale_list_users(output, host),
            HeadscaleCommands::ListNodes { output, host } => run_headscale_list_nodes(output, host),
            HeadscaleCommands::RemoveUser { name, yes, host } => {
                run_headscale_remove_user(name, yes, host)
            }
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
                tailnet_tag,
                no_input,
            } => run_host_add(AddHostArgs {
                name,
                address,
                user,
                port,
                ssh_key,
                tags,
                description,
                tailnet_tag,
                no_input,
            }),
            HostCommands::List { tags, output } => run_host_list(tags, output),
            HostCommands::Remove { name, yes } => run_host_remove(name, yes),
            HostCommands::Show { name } => run_host_show(name),
            HostCommands::Edit { name } => run_host_edit(name),
            HostCommands::Rename { old, new, yes } => run_host_rename(old, new, yes),
            HostCommands::DetectTailscaleIp { name } => run_host_detect_tailscale_ip(name),
        },
        Commands::Ansible(cmd) => match cmd {
            AnsibleCommands::Run {
                host,
                playbook,
                check,
                tags,
                skip_tags,
                user,
                ask_pass,
                force,
            } => signal::with_ctrlc(|| {
                run_ansible_run(
                    host, playbook, check, tags, skip_tags, user, ask_pass, force,
                )
            }),
            AnsibleCommands::Bootstrap {
                host,
                port,
                ip,
                user,
                force,
            } => run_ansible_bootstrap(host, port, ip, user, force),
        },
        Commands::Backup(cmd) => match cmd {
            BackupCommands::Create {
                host,
                apps,
                dest,
                ssh_key,
                include_music,
                dry_run,
            } => signal::with_ctrlc(|| {
                run_backup_create(
                    host,
                    apps,
                    dest,
                    ssh_key,
                    create_parameters(include_music),
                    dry_run,
                )
                .and_then(|outcome| {
                    let failed = outcome.failed_apps();
                    if failed.is_empty() {
                        Ok(())
                    } else {
                        eyre::bail!("{} backup(s) failed", failed.len());
                    }
                })
            }),
            BackupCommands::Sync {
                host,
                apps,
                ssh_key,
                include_music,
                dry_run,
            } => {
                signal::with_ctrlc(|| run_backup_sync(host, apps, ssh_key, include_music, dry_run))
            }
            BackupCommands::List { host, app, output } => run_backup_list(host, app, output),
            BackupCommands::Restore {
                backup_id,
                host,
                from_host,
                apps,
                ssh_key,
                dry_run,
                yes,
                skip_playbook_unsafe,
            } => signal::with_ctrlc(|| {
                run_backup_restore(RestoreOptions {
                    backup_id,
                    host_arg: host,
                    from_host_arg: from_host,
                    apps,
                    ssh_key,
                    dry_run,
                    yes,
                    skip_playbook_unsafe,
                })
            }),
            BackupCommands::Push { host, backup_id } => {
                signal::with_ctrlc(|| run_backup_push(host, backup_id))
            }
            BackupCommands::Prune { dry_run } => signal::with_ctrlc(|| run_backup_prune(dry_run)),
            BackupCommands::Verify {
                host,
                app,
                max_age,
                output,
            } => std::process::exit(run_backup_verify(VerifyOptions {
                host,
                app,
                max_age,
                format: output,
            })),
            BackupCommands::Opml(cmd) => match cmd {
                OpmlCommands::ExportOpml {
                    host,
                    output,
                    ssh_key,
                    user,
                } => run_export_opml(host, output, ssh_key, user),
                OpmlCommands::ImportOpml {
                    host,
                    input,
                    ssh_key,
                    user,
                } => run_import_opml(host, input, ssh_key, user),
            },
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
            } => signal::with_ctrlc(|| run_sync_music(host, source, dry_run)),
            SyncCommands::Hermes {
                host,
                source,
                dry_run,
                pull,
            } => signal::with_ctrlc(|| run_sync_hermes(host, source, dry_run, pull)),
        },
        Commands::Dns(cmd) => match cmd {
            DnsCommands::List { subdomain, output } => run_dns_list(subdomain, output).await,
            DnsCommands::Status { output } => run_dns_status(output).await,
            DnsCommands::Set { subdomain, ip } => run_dns_set(subdomain, ip).await,
            DnsCommands::Delete {
                subdomain,
                dry_run,
                output,
                production,
                yes,
            } => run_dns_delete(subdomain, dry_run, output, production, yes).await,
            DnsCommands::Migrate {
                ip,
                dry_run,
                output,
            } => run_dns_migrate(ip, dry_run, output).await,
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
            } => std::process::exit(
                run_dns_set_all(SetAllOptions {
                    host,
                    ip,
                    dry_run,
                    yes,
                    strict,
                    subdomains,
                    skip,
                    output,
                    continue_on_error,
                })
                .await,
            ),
        },
        Commands::Github(cmd) => match cmd {
            GithubCommands::Invite => run_github_invite(),
            GithubCommands::Verify { output } => std::process::exit(run_github_verify(output)?),
        },
        Commands::Config(cmd) => match cmd {
            ConfigCommands::Init(args) => run_config_init(args),
            ConfigCommands::Set { key, value } => run_config_set(key, value),
            ConfigCommands::Get { key, resolved } => run_config_get(key, resolved),
            ConfigCommands::List => run_config_list(),
            ConfigCommands::Remove { key } => run_config_remove(key),
            ConfigCommands::Edit => run_config_edit(),
            ConfigCommands::Path => run_config_path(),
        },
        Commands::Bichon(cmd) => {
            let code = run_bichon_command(cmd).await?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Commands::Versions(cmd) => std::process::exit(run_versions(cmd).await),
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "auberge",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    };

    // After the command, not before: whether a command routes to a Host is
    // only knowable once it has tried. A `--via` that decided nothing is
    // reported rather than ignored — a flag believed to have moved the route
    // and did not is how #780 started.
    outcome.and_then(|()| route::ensure_override_reached_a_host())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_bash_script() -> String {
        let mut buf = Vec::new();
        clap_complete::generate(
            clap_complete::Shell::Bash,
            &mut Cli::command(),
            "auberge",
            &mut buf,
        );
        String::from_utf8(buf).expect("bash completion script is valid UTF-8")
    }

    fn subcommands_missing_about(cmd: &clap::Command, path: &str) -> Vec<String> {
        cmd.get_subcommands()
            .filter(|sub| sub.get_name() != "help")
            .flat_map(|sub| {
                let sub_path = format!("{path} {}", sub.get_name());
                let mut missing = subcommands_missing_about(sub, &sub_path);
                if sub.get_about().is_none() {
                    missing.push(sub_path);
                }
                missing
            })
            .collect()
    }

    #[test]
    fn every_subcommand_has_a_description() {
        let missing = subcommands_missing_about(&Cli::command(), "auberge");
        assert!(
            missing.is_empty(),
            "subcommands render blank in --help: {}",
            missing.join(", ")
        );
    }

    #[test]
    fn bash_completion_covers_subcommands() {
        let script = generate_bash_script();
        for subcommand in [
            "deploy",
            "select",
            "ansible",
            "backup",
            "headscale",
            "host",
            "ssh",
            "sync",
            "dns",
            "config",
            "bichon",
            "versions",
            "completions",
        ] {
            assert!(
                script.contains(&format!("auberge,{subcommand})")),
                "bash completion misses subcommand {subcommand}"
            );
        }
    }

    #[test]
    fn bash_completion_covers_aliases() {
        let script = generate_bash_script();
        for alias in ["dp", "se", "a", "b", "hs", "h", "ss", "sy", "d", "c", "v"] {
            assert!(
                script.contains(&format!("auberge,{alias})")),
                "bash completion misses alias {alias}"
            );
        }
        for (parent, alias) in [("backup", "c"), ("ansible", "r"), ("dns", "sa")] {
            assert!(
                script.contains(&format!("__{parent},{alias})")),
                "bash completion misses nested alias {parent} {alias}"
            );
        }
    }

    #[test]
    fn bash_completion_covers_global_flags_and_output_enum() {
        let script = generate_bash_script();
        for flag in ["--verbose", "--quiet", "--no-color"] {
            assert!(
                script.contains(flag),
                "bash completion misses global flag {flag}"
            );
        }
        assert!(
            script.contains("human json"),
            "bash completion misses --output enum values"
        );
    }

    /// The OPML pair's CLI surface, read off the built `Command` tree rather
    /// than the Rust enum it derives from. #673 moved those two variants into
    /// their own module and flattened them back under `backup`, and a flatten
    /// that lands them somewhere else — or quietly drops an alias, a short, or
    /// a default — compiles clean and breaks only for whoever types it.
    ///
    /// `-o` here is a file path, not ADR-0004's `--output {human,json}`. That
    /// collision predates the move and is pinned because #673 requires the
    /// surface to be unchanged — pinned, not endorsed.
    #[test]
    fn the_opml_pair_stays_under_backup_with_its_surface() {
        let cli = Cli::command();
        let backup = cli
            .get_subcommands()
            .find(|sub| sub.get_name() == "backup")
            .expect("auberge backup must exist");

        for (name, alias, about, file_arg, file_short) in [
            (
                "export-opml",
                "eo",
                "Export FreshRSS feeds to OPML file",
                "output",
                'o',
            ),
            (
                "import-opml",
                "io",
                "Import OPML file to FreshRSS",
                "input",
                'i',
            ),
        ] {
            let cmd = backup
                .get_subcommands()
                .find(|sub| sub.get_name() == name)
                .unwrap_or_else(|| panic!("auberge backup {name} must exist"));

            assert_eq!(
                cmd.get_visible_aliases().collect::<Vec<_>>(),
                [alias],
                "auberge backup {name} lost its visible alias"
            );
            assert_eq!(
                cmd.get_about().map(|about| about.to_string()).as_deref(),
                Some(about),
                "auberge backup {name} lost its description"
            );

            let arg = |id: &str| {
                cmd.get_arguments()
                    .find(|arg| arg.get_id().as_str() == id)
                    .unwrap_or_else(|| panic!("auberge backup {name} has no {id} argument"))
            };
            assert_eq!(arg("host").get_short(), Some('H'));
            assert_eq!(arg("ssh_key").get_short(), Some('k'));
            assert_eq!(arg(file_arg).get_short(), Some(file_short));

            let user_default: Vec<_> = arg("user")
                .get_default_values()
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                user_default,
                ["admin"],
                "auberge backup {name} --user lost its default"
            );
        }
    }
    /// The other direction: the pair is reachable under `auberge backup` and
    /// nowhere else. A second `#[command(flatten)]`, or a promotion to top
    /// level that forgot to drop the old one, leaves every assertion above
    /// passing while `--help` grows a duplicate. ADR-0043's fences run both
    /// ways for the same reason.
    #[test]
    fn the_opml_pair_is_reachable_nowhere_but_backup() {
        fn walk(cmd: &clap::Command, prefix: &str, found: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                let path = format!("{prefix} {}", sub.get_name());
                if matches!(sub.get_name(), "export-opml" | "import-opml") {
                    found.push(path.clone());
                }
                walk(sub, &path, found);
            }
        }

        let mut found = Vec::new();
        walk(&Cli::command(), "auberge", &mut found);
        found.sort();
        assert_eq!(
            found,
            ["auberge backup export-opml", "auberge backup import-opml"],
            "the OPML pair is reachable somewhere other than `auberge backup`"
        );
    }
}
