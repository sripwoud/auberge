use crate::hosts::{Host, HostManager, TailnetTag};
use crate::output;
use crate::output::OutputFormat;
use crate::prompt::{Choice, confirm, select_item};
use crate::services::known_hosts;
use crate::services::ssh::{CONNECT_TIMEOUT, LiveSshSession, SshSession};
use clap::Subcommand;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use eyre::{Context, Result};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use tabled::Tabled;

pub struct AddHostArgs {
    pub name: Option<String>,
    pub address: Option<String>,
    pub user: Option<String>,
    pub port: u16,
    pub ssh_key: Option<String>,
    pub tags: Option<String>,
    pub description: Option<String>,
    pub tailnet_tag: Option<TailnetTag>,
    pub no_input: bool,
}

#[derive(Tabled)]
struct HostDisplay {
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "ADDRESS")]
    address: String,
    #[tabled(rename = "USER")]
    user: String,
    #[tabled(rename = "PORT")]
    port: u16,
    #[tabled(rename = "TAGS")]
    tags: String,
    /// The ADR-0055 trust tier, `-` when unset. Shown because an untagged node
    /// is exactly what stays invisible otherwise: four of five were.
    #[tabled(rename = "TIER")]
    tailnet_tag: String,
}

impl From<&Host> for HostDisplay {
    fn from(host: &Host) -> Self {
        Self {
            name: host.name.clone(),
            address: host.address.clone(),
            user: host.user.clone(),
            port: host.port,
            tags: host.tags.join(", "),
            tailnet_tag: host
                .tailnet_tag
                .map_or_else(|| "-".to_string(), |tier| tier.to_string()),
        }
    }
}

#[derive(Subcommand)]
pub enum HostCommands {
    #[command(visible_alias = "a", about = "Add a new host")]
    Add {
        #[arg(help = "Host name")]
        name: Option<String>,
        #[arg(help = "Host address (IP or hostname)")]
        address: Option<String>,
        #[arg(short, long, help = "SSH user")]
        user: Option<String>,
        #[arg(short, long, help = "SSH port", default_value = "22")]
        port: u16,
        #[arg(long, help = "Path to SSH key")]
        ssh_key: Option<String>,
        #[arg(short, long, help = "Tags (comma-separated)")]
        tags: Option<String>,
        #[arg(short, long, help = "Description")]
        description: Option<String>,
        #[arg(
            long,
            value_enum,
            help = "Tailnet trust tier (ADR-0055); omit to leave unset"
        )]
        tailnet_tag: Option<TailnetTag>,
        #[arg(long, help = "Disable interactive prompts")]
        no_input: bool,
    },
    #[command(visible_alias = "l", about = "List all hosts")]
    List {
        #[arg(short, long, help = "Filter by tags (comma-separated)")]
        tags: Option<String>,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
    },
    #[command(visible_alias = "rm", about = "Remove a host")]
    Remove {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
        #[arg(short, long, help = "Skip confirmation")]
        yes: bool,
    },
    #[command(visible_alias = "s", about = "Show host details")]
    Show {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
    },
    #[command(visible_alias = "e", about = "Edit a host")]
    Edit {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
    },
    #[command(
        visible_alias = "mv",
        about = "Rename a host: remote hostname, hosts.toml entry, and key directory"
    )]
    Rename {
        #[arg(help = "Current host name")]
        old: String,
        #[arg(help = "New host name")]
        new: String,
        #[arg(short, long, help = "Skip confirmation")]
        yes: bool,
    },
    #[command(
        visible_alias = "dti",
        about = "Detect and cache the host's Tailscale IPv4 (queries the host via SSH)"
    )]
    DetectTailscaleIp {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
    },
}

pub fn run_host_add(args: AddHostArgs) -> Result<()> {
    let is_tty = HostManager::is_tty();
    let interactive = is_tty && !args.no_input;

    let ssh_config_hosts = if interactive {
        match crate::ssh_config::SshConfigParser::new().and_then(|p| p.parse()) {
            Ok(hosts) if !hosts.is_empty() => {
                let existing_hosts = HostManager::list_hosts_filtered(None).unwrap_or_default();
                let existing_names: Vec<String> =
                    existing_hosts.iter().map(|h| h.name.clone()).collect();

                let available_hosts: Vec<_> = hosts
                    .into_iter()
                    .filter(|h| !existing_names.contains(&h.name))
                    .collect();

                if available_hosts.is_empty() {
                    None
                } else {
                    Some(available_hosts)
                }
            }
            Ok(_) => None,
            Err(e) => {
                output::info(&format!("Could not parse SSH config: {}", e));
                None
            }
        }
    } else {
        None
    };

    let imported_host = if let Some(ref ssh_hosts) = ssh_config_hosts {
        output::info(&format!(
            "Found {} new host(s) in ~/.ssh/config",
            ssh_hosts.len()
        ));

        let mut options: Vec<crate::ssh_config::SshConfigHost> =
            vec![crate::ssh_config::SshConfigHost {
                name: "Enter manually".to_string(),
                hostname: None,
                user: None,
                port: None,
                identity_file: None,
            }];
        options.extend(ssh_hosts.clone());

        // Dismissing the picker means "enter manually", same as the sentinel
        // entry: `host add` has a manual path, so an abort is not an error.
        select_item(
            &options,
            |h: &crate::ssh_config::SshConfigHost| match &h.hostname {
                None => "Enter manually".to_string(),
                Some(addr) => {
                    let port = h.port.unwrap_or(22);
                    format!("{} ({}:{})", h.name, addr, port)
                }
            },
            Choice::new("import source").with_prompt("Import from SSH config or enter manually?"),
        )
        .ok()
        .filter(|h| h.hostname.is_some())
    } else {
        None
    };

    let (name, address, user, port, ssh_key) = if let Some(imported) = imported_host {
        let name = imported.name;
        let address = imported.hostname.unwrap();
        let default_user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
        let user = imported.user.unwrap_or(default_user);
        let port = imported.port.unwrap_or(22);

        let ssh_key = imported.identity_file.and_then(|path| {
            let expanded = shellexpand::tilde(&path).into_owned();
            let key_path = PathBuf::from(&expanded);
            if !key_path.exists() {
                output::info(&format!(
                    "SSH key not found: {} (will use default derivation)",
                    expanded
                ));
                None
            } else {
                Some(expanded)
            }
        });

        output::info(&format!(
            "Importing: {} -> {}@{}:{}",
            name, user, address, port
        ));
        (name, address, user, port, ssh_key.or(args.ssh_key))
    } else {
        let name = if let Some(n) = args.name {
            n
        } else if interactive {
            Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Host name")
                .interact_text()?
        } else {
            eyre::bail!("Host name is required (use --no-input in non-interactive mode)");
        };

        let address = if let Some(a) = args.address {
            a
        } else if interactive {
            Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Host address (IP or hostname)")
                .interact_text()?
        } else {
            eyre::bail!("Host address is required");
        };

        let default_user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
        let user = if let Some(u) = args.user {
            u
        } else if interactive {
            Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("SSH user")
                .default(default_user)
                .interact_text()?
        } else {
            default_user
        };

        (name, address, user, args.port, args.ssh_key)
    };

    let tags_vec = args
        .tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let tailnet_tag = match args.tailnet_tag {
        Some(tier) => Some(tier),
        None if interactive => prompt_tailnet_tag(None)?,
        None => None,
    };

    if tailnet_tag.is_none() {
        output::info(
            "No tailnet trust tier set. `auberge host edit` sets one; until then the host has no \
             place in the tailnet ACL policy.",
        );
    }

    let host = Host {
        name: name.clone(),
        address,
        user,
        port,
        ssh_key,
        tags: tags_vec,
        description: args.description,
        python_interpreter: None,
        become_method: "sudo".to_string(),
        tailscale_ip: None,
        tailnet_tag,
        unknown: toml::Table::new(),
    };

    HostManager::add_host(host)?;

    let config_path = HostManager::config_path()?;
    output::success(&format!(
        "Host '{}' added to {}",
        name,
        config_path.display()
    ));
    sync_ssh_include()?;

    Ok(())
}

pub fn run_host_list(tags: Option<String>, output: OutputFormat) -> Result<()> {
    let filter_tags = tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

    let hosts = HostManager::list_hosts_filtered(filter_tags)?;

    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&hosts)?);
        }
        OutputFormat::Human => {
            if hosts.is_empty() {
                output::info("No hosts configured yet");
                eprintln!();
                eprintln!("Add a host with:");
                eprintln!("  auberge host add <name> <address>");
                return Ok(());
            }
            let display_hosts: Vec<HostDisplay> = hosts.iter().map(HostDisplay::from).collect();
            output::print_table(&display_hosts);
        }
    }

    Ok(())
}

pub fn run_host_remove(name: Option<String>, yes: bool) -> Result<()> {
    let host = crate::hosts::select_or_arg(name, crate::hosts::HOST_POSITIONAL)?;
    if !confirm(&format!("Remove host '{}'?", host.name), yes) {
        eprintln!("Cancelled.");
        return Ok(());
    }

    HostManager::remove_host(&host.name)?;
    output::success(&format!("Host '{}' removed", host.name));
    sync_ssh_include()?;

    Ok(())
}

pub fn run_host_show(name: Option<String>) -> Result<()> {
    let host = crate::hosts::select_or_arg(name, crate::hosts::HOST_POSITIONAL)?;
    println!("{}", serde_yaml::to_string(&host)?);
    Ok(())
}

pub fn run_host_detect_tailscale_ip(name_arg: Option<String>) -> Result<()> {
    let host = crate::hosts::select_or_arg(name_arg, crate::hosts::HOST_POSITIONAL)?;
    let ssh_key = resolve_ssh_key(&host)?;
    let route = crate::services::route::resolve(&host, Some(ssh_key));
    let session = LiveSshSession::new(&route, &host.become_method)?;

    output::info(&format!(
        "Querying Tailscale IPv4 on {}@{}…",
        route.user, route.address
    ));

    let detected = detect_tailscale_ip(&session, &host.name)?;

    let mut updated = host.clone();
    updated.tailscale_ip = Some(detected.clone());
    HostManager::update_host(&host.name, updated)?;

    output::success(&format!(
        "Cached tailscale_ip={} for host '{}'",
        detected, host.name
    ));
    Ok(())
}

/// Regenerates the CLI-owned ~/.ssh/config.d/auberge.conf from the hosts.toml
/// on disk after every host mutation (#534). The user's ~/.ssh/config is never
/// written; when it lacks the Include line, only a hint is printed.
fn sync_ssh_include() -> Result<()> {
    let hosts = HostManager::load_hosts()?;
    migrate_known_hosts_aliases(&hosts)?;
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    let ssh_dir = home.join(".ssh");
    crate::services::ssh_include::write_include_file(&ssh_dir, &hosts).wrap_err(
        "hosts.toml was updated but ~/.ssh/config.d/auberge.conf could not be regenerated; rerun any host subcommand after fixing",
    )?;
    if !crate::services::ssh_include::main_config_has_include(&ssh_dir)? {
        output::info(
            "ssh aliases inactive: add this line at the top of ~/.ssh/config (first-obtained value wins):",
        );
        output::info(&format!("  {}", crate::services::ssh_include::INCLUDE_LINE));
    }
    Ok(())
}

/// Copies each host's already-verified known_hosts key onto its
/// `HostKeyAlias` (#785) before the include can start advertising one, so
/// `StrictHostKeyChecking accept-new` never silently re-trusts a host this
/// roster already knows.
///
/// Deliberately scoped to this one choke point rather than every ssh/scp/
/// rsync call site: every live connection already sends `-o
/// HostKeyAlias=<name>` unconditionally (`SshTransport`,
/// `ansible_ssh_extra_args`), so a host that has never gone through
/// `add`/`edit`/`rename`/`remove` since upgrading to this version stays
/// unmigrated — its first post-upgrade connection accept-news under the
/// alias like a fresh host, exactly once, until one of those commands
/// runs for it. Run any host mutation for the whole roster right after
/// upgrading if that gap matters to you.
fn migrate_known_hosts_aliases(hosts: &[Host]) -> Result<()> {
    for host in hosts {
        let legacy_target = known_hosts::legacy_target(&host.address, host.port);
        known_hosts::migrate_alias(&host.name, &legacy_target).wrap_err_with(|| {
            format!(
                "Failed to migrate the known_hosts alias for host '{}'",
                host.name
            )
        })?;
    }
    Ok(())
}

fn resolve_ssh_key(host: &Host) -> Result<PathBuf> {
    let key = match host.ssh_key.as_ref() {
        Some(p) => crate::services::ssh::configured_key_path(p),
        None => crate::services::ssh::default_ssh_key_path(&host.user, &host.name)?,
    };
    if !key.exists() {
        eyre::bail!(
            "SSH key not found: {}. Run 'auberge ssh keygen --host {} --user {}' first.",
            key.display(),
            host.name,
            host.user
        );
    }
    Ok(key)
}

/// The Host's own Tailscale CGNAT address, as `tailscale ip -4` reports it.
fn detect_tailscale_ip(session: &dyn SshSession, host_name: &str) -> Result<String> {
    let out = session.run("tailscale ip -4")?;
    if !out.success {
        let stderr = out.stderr_str();
        let stderr = stderr.trim();
        if stderr.is_empty() {
            eyre::bail!("`tailscale ip -4` failed on {}", host_name);
        }
        eyre::bail!("`tailscale ip -4` failed on {}: {}", host_name, stderr);
    }

    let stdout = out.stdout_str();
    parse_tailscale_cgnat_ipv4(&stdout).ok_or_else(|| {
        eyre::eyre!(
            "No Tailscale CGNAT IPv4 found in `tailscale ip -4` output for {}: {:?}",
            host_name,
            stdout.trim()
        )
    })
}

fn parse_tailscale_cgnat_ipv4(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .find_map(|line| {
            let addr = line.parse::<Ipv4Addr>().ok()?;
            is_cgnat_ipv4(&addr).then(|| addr.to_string())
        })
}

fn is_cgnat_ipv4(addr: &Ipv4Addr) -> bool {
    let octets = addr.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn none_if_empty(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

/// The unset entry the tier picker offers first.
const NO_TIER: &str = "(none)";

/// The picker's items: the unset entry, then every tier in ADR-0055's order.
fn tier_items() -> Vec<String> {
    let mut items = vec![NO_TIER.to_string()];
    items.extend(TailnetTag::ALL.iter().map(ToString::to_string));
    items
}

/// Where `current` sits in [`tier_items`]. Index 0 is [`NO_TIER`], so a tier is
/// its position in `TailnetTag::ALL` shifted by one.
fn tier_item_index(current: Option<TailnetTag>) -> usize {
    current
        .and_then(|tier| TailnetTag::ALL.iter().position(|t| *t == tier))
        .map_or(0, |index| index + 1)
}

/// The inverse of [`tier_item_index`]: which tier item `index` selects.
fn tier_at_item(index: usize) -> Option<TailnetTag> {
    index.checked_sub(1).map(|index| TailnetTag::ALL[index])
}

/// Pick a Host's trust tier from the closed ADR-0055 set, `current` preselected.
///
/// A `Select` and not an `Input`, because the set is closed and a free-text
/// typo would surface as a `hosts.toml` parse failure on some later, unrelated
/// command rather than here. Unset is an entry rather than an empty string
/// because `.default()` on a dialoguer `Input` is not clearable — the same trap
/// `ssh_key` works around with `with_initial_text`.
///
/// The off-by-one that entry costs is the whole correctness of the picker and
/// is unreachable through a `Select`, so it lives in [`tier_item_index`] and
/// [`tier_at_item`], which are tested as a round trip.
fn prompt_tailnet_tag(current: Option<TailnetTag>) -> Result<Option<TailnetTag>> {
    let picked = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Tailnet trust tier")
        .items(&tier_items())
        .default(tier_item_index(current))
        .interact()?;

    Ok(tier_at_item(picked))
}

pub fn run_host_edit(name: Option<String>) -> Result<()> {
    let host = crate::hosts::select_or_arg(name, crate::hosts::HOST_POSITIONAL)?;

    let address = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Host address")
        .default(host.address.clone())
        .interact_text()?;

    let user = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("SSH user")
        .default(host.user.clone())
        .interact_text()?;

    let port = Input::<u16>::with_theme(&ColorfulTheme::default())
        .with_prompt("SSH port")
        .default(host.port)
        .interact_text()?;

    let ssh_key = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("SSH key (empty for derived default)")
        .with_initial_text(host.ssh_key.clone().unwrap_or_default())
        .allow_empty(true)
        .interact_text()?;

    let tags_str = host.tags.join(", ");
    let new_tags_str = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Tags (comma-separated)")
        .default(tags_str)
        .allow_empty(true)
        .interact_text()?;

    let tags_vec: Vec<String> = if new_tags_str.is_empty() {
        Vec::new()
    } else {
        new_tags_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };

    let description = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Description")
        .default(host.description.clone().unwrap_or_default())
        .allow_empty(true)
        .interact_text()?;

    let tailnet_tag = prompt_tailnet_tag(host.tailnet_tag)?;

    let updated_host = Host {
        name: host.name.clone(),
        address,
        user,
        port,
        ssh_key: none_if_empty(ssh_key),
        tags: tags_vec,
        description: none_if_empty(description),
        python_interpreter: host.python_interpreter,
        become_method: host.become_method,
        tailscale_ip: host.tailscale_ip,
        tailnet_tag,
        unknown: host.unknown,
    };

    HostManager::update_host(&host.name, updated_host)?;
    output::success(&format!("Host '{}' updated", host.name));
    sync_ssh_include()?;

    Ok(())
}

pub fn run_host_rename(old: String, new: String, yes: bool) -> Result<()> {
    validate_rename_name(&old)?;
    validate_rename_name(&new)?;

    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    let identities = crate::services::ssh::identities_dir(&home);

    let hosts = HostManager::load_hosts()?;
    let host = preflight_names(&hosts, &old, &new)?;
    preflight_identities(&identities, &old, &new)?;
    let mut config = load_config_if_present()?;
    if let Some(config) = &config
        && config.host_override_names().contains(&old.to_string())
        && config.host_override_names().contains(&new.to_string())
    {
        eyre::bail!(
            "config.toml holds both [hosts.{old}] and [hosts.{new}]; merge them before renaming"
        );
    }

    let ssh_key = rename_key_candidates(&host, &home, &new)
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| {
            eyre::eyre!(
                "SSH key not found for '{}'. Run 'auberge ssh keygen --host {} --user {}' first.",
                host.name,
                host.name,
                host.user
            )
        })?;

    let route = crate::services::route::resolve(&host, Some(ssh_key));
    let session = LiveSshSession::new(&route, &host.become_method)?;
    session.reachable(CONNECT_TIMEOUT)?;

    if !confirm(
        &format!(
            "Rename host '{}' -> '{}' (sets the remote hostname and rewrites local config)?",
            old, new
        ),
        yes,
    ) {
        eprintln!("Cancelled.");
        return Ok(());
    }

    rename_remote(&session, &old, &new)?;

    let updated = rename_local(&identities, hosts, &old, &new, &home)?;
    if let Some(config) = &mut config
        && config.rename_host_overrides(&old, &new)?
    {
        output::info(&format!(
            "config.toml: moved [hosts.{old}] to [hosts.{new}]"
        ));
    }
    HostManager::save_hosts(&updated)?;

    output::success(&format!("Host '{}' renamed to '{}'", old, new));
    sync_ssh_include()?;
    print_rename_follow_ups(&old, &new);
    Ok(())
}

/// The config file is optional for a rename — a fleet without one has no
/// override tables to carry — but a present-and-unreadable one must stop the
/// rename rather than silently orphan its `[hosts.<name>]` answers.
fn load_config_if_present() -> Result<Option<crate::config::Config>> {
    match crate::config::Config::load() {
        Ok(config) => Ok(Some(config)),
        Err(_) if !crate::config::Config::path()?.exists() => Ok(None),
        Err(err) => Err(err),
    }
}

fn validate_rename_name(name: &str) -> Result<()> {
    let starts_alphanumeric = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    let charset_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !starts_alphanumeric || !charset_ok {
        eyre::bail!(
            "Invalid host name '{}': must start with a letter or digit and contain only letters, digits, '.', '_' and '-'",
            name
        );
    }
    Ok(())
}

fn preflight_names(hosts: &[Host], old: &str, new: &str) -> Result<Host> {
    let host = hosts
        .iter()
        .find(|h| h.name == old)
        .cloned()
        .ok_or_else(|| eyre::eyre!("Host '{}' not found", old))?;
    if hosts.iter().any(|h| h.name == new) {
        eyre::bail!("Host '{}' already exists", new);
    }
    Ok(host)
}

/// A pre-existing `identities/<new>` blocks the rename only while
/// `identities/<old>` also exists (the mv would clobber it). With `<old>`
/// gone, `<new>` is the already-moved state a rerun recovers through
/// (ADR-0024).
fn preflight_identities(identities: &std::path::Path, old: &str, new: &str) -> Result<()> {
    let old_dir = identities.join(old);
    let new_dir = identities.join(new);
    if old_dir.exists() && new_dir.exists() {
        eyre::bail!(
            "Both {} and {} exist; move the keys out of one before renaming",
            old_dir.display(),
            new_dir.display()
        );
    }
    Ok(())
}

/// Rewrites a configured ssh_key iff it lives under `~/.ssh/identities/<old>/`,
/// preserving the raw prefix style (`~` or absolute). A custom key outside the
/// derived tree returns None: file and configured path stay untouched (#520).
fn rewrite_ssh_key(raw: &str, home: &std::path::Path, old: &str, new: &str) -> Option<String> {
    let expanded = match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(raw),
    };
    let identities = crate::services::ssh::identities_dir(home);
    let rest = expanded
        .strip_prefix(identities.join(old))
        .ok()?
        .to_path_buf();
    let rewritten = identities.join(new).join(rest);
    if raw.starts_with("~/") {
        let relative = rewritten.strip_prefix(home).ok()?;
        Some(format!("~/{}", relative.display()))
    } else {
        Some(rewritten.display().to_string())
    }
}

/// After a partial rename the key may already live under the new name while
/// hosts.toml still says old, so the preflight probes both locations.
fn rename_key_candidates(host: &Host, home: &std::path::Path, new: &str) -> Vec<PathBuf> {
    match &host.ssh_key {
        Some(raw) => {
            let mut candidates = vec![crate::services::ssh::configured_key_path(raw)];
            if let Some(rewritten) = rewrite_ssh_key(raw, home, &host.name, new) {
                candidates.push(crate::services::ssh::configured_key_path(&rewritten));
            }
            candidates
        }
        None => {
            let [old_dir, _] = crate::services::ssh::identity_scan_dirs(home, &host.name);
            let [new_dir, _] = crate::services::ssh::identity_scan_dirs(home, new);
            vec![old_dir.join(&host.user), new_dir.join(&host.user)]
        }
    }
}

fn sed_hostname_pattern(name: &str) -> String {
    name.replace('.', "\\.")
}

fn etc_hosts_sed_script(old: &str, new: &str) -> String {
    let pattern = sed_hostname_pattern(old);
    format!("s/(^|[[:space:]]){pattern}([[:space:]]|\\.|$)/\\1{new}\\2/g")
}

fn rename_remote_commands(old: &str, new: &str) -> [String; 2] {
    let script = etc_hosts_sed_script(old, new);
    // The substitution runs twice: sed resumes scanning after each
    // replacement, so a boundary consumed as `\2` hides an adjacent
    // occurrence from the first pass.
    [
        format!("sudo hostnamectl set-hostname {new}"),
        format!("sudo sed -E -i -e '{script}' -e '{script}' /etc/hosts"),
    ]
}

fn rename_remote(
    session: &impl crate::services::ssh::SshSession,
    old: &str,
    new: &str,
) -> Result<()> {
    for command in rename_remote_commands(old, new) {
        let result = session.run(&command)?;
        if !result.success {
            let stderr = result.stderr_str();
            let stderr = stderr.trim();
            eyre::bail!(
                "Remote step failed ({}): {}. No local changes were made; rerun after fixing.",
                command,
                if stderr.is_empty() {
                    "no output"
                } else {
                    stderr
                }
            );
        }
    }
    Ok(())
}

/// Local mutations ordered so the hosts.toml write (done by the caller) is the
/// commit point: the key-directory move happens first and is skipped when
/// already done, so a rerun after any partial failure converges (ADR-0024).
fn rename_local(
    identities: &std::path::Path,
    hosts: Vec<Host>,
    old: &str,
    new: &str,
    home: &std::path::Path,
) -> Result<Vec<Host>> {
    let old_dir = identities.join(old);
    if old_dir.exists() {
        let new_dir = identities.join(new);
        std::fs::rename(&old_dir, &new_dir).wrap_err_with(|| {
            format!(
                "Failed to move {} to {}",
                old_dir.display(),
                new_dir.display()
            )
        })?;
    }

    Ok(hosts
        .into_iter()
        .map(|mut h| {
            if h.name == old {
                h.name = new.to_string();
                if let Some(rewritten) = h
                    .ssh_key
                    .as_deref()
                    .and_then(|raw| rewrite_ssh_key(raw, home, old, new))
                {
                    h.ssh_key = Some(rewritten);
                }
            }
            h
        })
        .collect())
}

fn print_rename_follow_ups(old: &str, new: &str) {
    output::info("Not done by this command:");
    output::info(&format!(
        "  - tailscale: the host re-advertises itself as '{new}' and releases the '{old}' tailnet name"
    ));
    output::warn(&format!(
        "restic snapshots group by host name: the '{old}' lineage freezes here and '{new}' starts a new one — never rewrite snapshot tags"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgnat_classification() {
        assert!(is_cgnat_ipv4(&"100.64.0.1".parse().unwrap()));
        assert!(is_cgnat_ipv4(&"100.99.62.26".parse().unwrap()));
        assert!(is_cgnat_ipv4(&"100.127.255.254".parse().unwrap()));

        assert!(!is_cgnat_ipv4(&"100.63.255.255".parse().unwrap()));
        assert!(!is_cgnat_ipv4(&"100.128.0.0".parse().unwrap()));
        assert!(!is_cgnat_ipv4(&"10.0.0.1".parse().unwrap()));
        assert!(!is_cgnat_ipv4(&"192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn parses_first_cgnat_ipv4_from_tailscale_output() {
        let stdout = "100.99.62.26\n";
        assert_eq!(
            parse_tailscale_cgnat_ipv4(stdout),
            Some("100.99.62.26".to_string())
        );
    }

    #[test]
    fn skips_blank_lines_and_non_cgnat_lines() {
        let stdout = "\n203.0.113.10\n100.99.62.26\nfd7a:115c:a1e0::1\n";
        assert_eq!(
            parse_tailscale_cgnat_ipv4(stdout),
            Some("100.99.62.26".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_cgnat_present() {
        assert_eq!(parse_tailscale_cgnat_ipv4(""), None);
        assert_eq!(parse_tailscale_cgnat_ipv4("203.0.113.10\n"), None);
        assert_eq!(
            parse_tailscale_cgnat_ipv4("not-an-ip\nfd7a:115c:a1e0::1\n"),
            None
        );
    }

    #[test]
    fn none_if_empty_maps_empty_input_to_none() {
        assert_eq!(none_if_empty(String::new()), None);
        assert_eq!(
            none_if_empty("~/.ssh/identities/custom".to_string()),
            Some("~/.ssh/identities/custom".to_string())
        );
    }

    #[test]
    fn host_commands_error_on_unknown_name() {
        let unknown = || Some("__nonexistent_host__".to_string());

        assert!(run_host_show(unknown()).is_err());
        assert!(run_host_remove(unknown(), true).is_err());
        assert!(run_host_edit(unknown()).is_err());
    }

    fn rename_fixture_host(name: &str, ssh_key: Option<&str>) -> Host {
        Host {
            name: name.to_string(),
            address: "203.0.113.10".to_string(),
            user: "ansible".to_string(),
            port: 22,
            ssh_key: ssh_key.map(str::to_string),
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
            tailnet_tag: None,
            unknown: toml::Table::new(),
        }
    }

    #[test]
    fn validate_rename_name_accepts_hostname_shapes() {
        assert!(validate_rename_name("auberge").is_ok());
        assert!(validate_rename_name("vieille-auberge").is_ok());
        assert!(validate_rename_name("vps.example.com").is_ok());
        assert!(validate_rename_name("box_2").is_ok());
    }

    #[test]
    fn validate_rename_name_rejects_shell_hostile_names() {
        assert!(validate_rename_name("").is_err());
        assert!(validate_rename_name("-flag").is_err());
        assert!(validate_rename_name("a b").is_err());
        assert!(validate_rename_name("a'b").is_err());
        assert!(validate_rename_name("a/b").is_err());
        assert!(validate_rename_name("a&b").is_err());
    }

    #[test]
    fn preflight_names_rejects_missing_old_and_taken_new() {
        let hosts = vec![
            rename_fixture_host("auberge", None),
            rename_fixture_host("relais", None),
        ];

        assert!(preflight_names(&hosts, "ghost", "x").is_err());
        assert!(preflight_names(&hosts, "auberge", "relais").is_err());

        let found = preflight_names(&hosts, "auberge", "vieille-auberge").unwrap();
        assert_eq!(found.name, "auberge");
    }

    #[test]
    fn preflight_identities_bails_only_on_true_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let identities = tmp.path();

        assert!(preflight_identities(identities, "old", "new").is_ok());

        std::fs::create_dir_all(identities.join("new")).unwrap();
        assert!(
            preflight_identities(identities, "old", "new").is_ok(),
            "new-only is the already-moved rerun state"
        );

        std::fs::create_dir_all(identities.join("old")).unwrap();
        assert!(
            preflight_identities(identities, "old", "new").is_err(),
            "both dirs present is a clobbering collision"
        );

        std::fs::remove_dir(identities.join("new")).unwrap();
        assert!(preflight_identities(identities, "old", "new").is_ok());
    }

    #[test]
    fn rewrite_ssh_key_rewrites_derived_tree_paths_preserving_style() {
        let home = std::path::Path::new("/home/x");

        assert_eq!(
            rewrite_ssh_key(
                "~/.ssh/identities/auberge/ansible",
                home,
                "auberge",
                "vieille-auberge"
            ),
            Some("~/.ssh/identities/vieille-auberge/ansible".to_string())
        );
        assert_eq!(
            rewrite_ssh_key(
                "/home/x/.ssh/identities/auberge/ansible",
                home,
                "auberge",
                "vieille-auberge"
            ),
            Some("/home/x/.ssh/identities/vieille-auberge/ansible".to_string())
        );
        assert_eq!(
            rewrite_ssh_key("~/.ssh/identities/auberge/sub/key", home, "auberge", "next"),
            Some("~/.ssh/identities/next/sub/key".to_string())
        );
    }

    #[test]
    fn rewrite_ssh_key_leaves_custom_paths_untouched() {
        let home = std::path::Path::new("/home/x");

        assert_eq!(
            rewrite_ssh_key("~/.ssh/custom/key", home, "auberge", "n"),
            None
        );
        assert_eq!(
            rewrite_ssh_key("~/.ssh/identities/other/ansible", home, "auberge", "n"),
            None,
            "another host's tree is not ours to rewrite"
        );
        assert_eq!(
            rewrite_ssh_key("~/.ssh/identities/github", home, "auberge", "n"),
            None,
            "flat service keys live directly under identities/"
        );
        assert_eq!(
            rewrite_ssh_key("/etc/keys/ansible", home, "auberge", "n"),
            None
        );
    }

    #[test]
    fn rename_key_candidates_probe_old_then_new_location() {
        let home = std::path::Path::new("/home/x");

        let derived = rename_fixture_host("auberge", None);
        assert_eq!(
            rename_key_candidates(&derived, home, "vieille-auberge"),
            vec![
                PathBuf::from("/home/x/.ssh/identities/auberge/ansible"),
                PathBuf::from("/home/x/.ssh/identities/vieille-auberge/ansible"),
            ]
        );

        let custom = rename_fixture_host("auberge", Some("/etc/keys/ansible"));
        assert_eq!(
            rename_key_candidates(&custom, home, "vieille-auberge"),
            vec![PathBuf::from("/etc/keys/ansible")],
            "a custom key has no rewritten twin to probe"
        );

        let configured =
            rename_fixture_host("auberge", Some("/home/x/.ssh/identities/auberge/ansible"));
        assert_eq!(
            rename_key_candidates(&configured, home, "vieille-auberge"),
            vec![
                PathBuf::from("/home/x/.ssh/identities/auberge/ansible"),
                PathBuf::from("/home/x/.ssh/identities/vieille-auberge/ansible"),
            ]
        );
    }

    #[test]
    fn rename_remote_commands_set_hostname_then_patch_etc_hosts() {
        let [hostnamectl, sed] = rename_remote_commands("auberge", "vieille-auberge");
        assert_eq!(hostnamectl, "sudo hostnamectl set-hostname vieille-auberge");
        let script = "s/(^|[[:space:]])auberge([[:space:]]|\\.|$)/\\1vieille-auberge\\2/g";
        assert_eq!(
            sed,
            format!("sudo sed -E -i -e '{script}' -e '{script}' /etc/hosts")
        );
    }

    #[test]
    fn rename_remote_commands_escape_dots_in_the_sed_pattern() {
        let [_, sed] = rename_remote_commands("a.example.com", "b");
        assert!(sed.contains("a\\.example\\.com"), "{sed}");
    }

    fn run_etc_hosts_sed(script: &str, input: &str) -> String {
        use std::io::Write;
        let mut child = std::process::Command::new("sed")
            .args(["-E", "-e", script, "-e", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    #[test]
    fn etc_hosts_sed_renames_adjacent_tokens_fqdns_and_line_ends() {
        let script = etc_hosts_sed_script("auberge", "relais");
        let input = "127.0.1.1 auberge auberge.lan auberge\n\
                     10.0.0.2 auberge.example.com\n\
                     auberge 10.0.0.3\n";
        assert_eq!(
            run_etc_hosts_sed(&script, input),
            "127.0.1.1 relais relais.lan relais\n\
             10.0.0.2 relais.example.com\n\
             relais 10.0.0.3\n"
        );
    }

    #[test]
    fn etc_hosts_sed_leaves_hyphenated_supersets_alone() {
        let script = etc_hosts_sed_script("auberge", "relais");
        let input = "10.0.0.2 vieille-auberge auberge-next\n";
        assert_eq!(run_etc_hosts_sed(&script, input), input);
    }

    /// The `(none)` entry shifts every tier by one, and the shift is applied in
    /// two places that must agree. A round trip over the whole domain — unset
    /// plus every tier — is the assertion, because an off-by-one here silently
    /// assigns a Host the wrong trust tier.
    #[test]
    fn a_tier_survives_the_pickers_index_shift() {
        let mut round_tripped = vec![tier_at_item(tier_item_index(None))];
        for tier in TailnetTag::ALL {
            round_tripped.push(tier_at_item(tier_item_index(Some(tier))));
        }
        assert_eq!(
            round_tripped,
            [
                None,
                Some(TailnetTag::Trusted),
                Some(TailnetTag::Data),
                Some(TailnetTag::Agent),
                Some(TailnetTag::Standby),
            ]
        );
    }

    /// Each item index the round trip above relies on must be a real item, or
    /// the `Select` would offer fewer entries than the mapping addresses.
    #[test]
    fn every_tier_has_an_item_to_select() {
        let items = tier_items();
        assert_eq!(items.len(), TailnetTag::ALL.len() + 1);
        assert_eq!(items[0], NO_TIER);
        for tier in TailnetTag::ALL {
            assert_eq!(items[tier_item_index(Some(tier))], tier.to_string());
        }
    }

    #[test]
    fn detect_tailscale_ip_asks_the_host_for_its_v4_address() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "100.101.255.46\n",
        ));
        assert_eq!(
            detect_tailscale_ip(&mock, "auberge").unwrap(),
            "100.101.255.46"
        );
        assert_eq!(
            mock.calls(),
            vec![crate::services::ssh::SshOp::Run(
                "tailscale ip -4".to_string()
            )]
        );
    }

    #[test]
    fn detect_tailscale_ip_rejects_a_non_cgnat_answer() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "192.168.1.10\n",
        ));
        let err = detect_tailscale_ip(&mock, "auberge").unwrap_err();
        assert!(err.to_string().contains("No Tailscale CGNAT IPv4"), "{err}");
    }

    #[test]
    fn detect_tailscale_ip_reports_the_remote_stderr() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(127),
            stdout: Vec::new(),
            stderr: b"tailscale: command not found".to_vec(),
        });
        let err = detect_tailscale_ip(&mock, "auberge").unwrap_err();
        assert_eq!(
            err.to_string(),
            "`tailscale ip -4` failed on auberge: tailscale: command not found"
        );
    }

    #[test]
    fn rename_remote_runs_both_steps_in_order() {
        let mock = crate::services::ssh::MockSshSession::new();
        rename_remote(&mock, "auberge", "vieille-auberge").unwrap();

        let expected: Vec<crate::services::ssh::SshOp> =
            rename_remote_commands("auberge", "vieille-auberge")
                .into_iter()
                .map(crate::services::ssh::SshOp::Run)
                .collect();
        assert_eq!(mock.calls(), expected);
    }

    #[test]
    fn rename_remote_aborts_on_first_failing_step() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"hostnamectl: access denied".to_vec(),
        });

        let err = rename_remote(&mock, "auberge", "vieille-auberge").unwrap_err();
        assert!(err.to_string().contains("access denied"), "{err}");
        assert_eq!(mock.calls().len(), 1, "second step must not run");
    }

    #[test]
    fn rename_local_moves_key_dir_and_rewrites_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let identities = tmp.path().join(".ssh/identities");
        std::fs::create_dir_all(identities.join("auberge")).unwrap();
        std::fs::write(identities.join("auberge/ansible"), b"key").unwrap();

        let raw_key = identities.join("auberge/ansible").display().to_string();
        let hosts = vec![
            rename_fixture_host("auberge", Some(&raw_key)),
            rename_fixture_host("relais", None),
        ];

        let updated =
            rename_local(&identities, hosts, "auberge", "vieille-auberge", tmp.path()).unwrap();

        assert!(!identities.join("auberge").exists());
        assert!(identities.join("vieille-auberge/ansible").exists());
        assert_eq!(updated[0].name, "vieille-auberge");
        assert_eq!(
            updated[0].ssh_key.as_deref(),
            Some(
                identities
                    .join("vieille-auberge/ansible")
                    .display()
                    .to_string()
            )
            .as_deref()
        );
        assert_eq!(updated[1].name, "relais", "other entries untouched");
    }

    #[test]
    fn rename_local_leaves_custom_key_file_and_path_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let identities = tmp.path().join(".ssh/identities");
        std::fs::create_dir_all(&identities).unwrap();

        let hosts = vec![rename_fixture_host("auberge", Some("/etc/keys/ansible"))];
        let updated = rename_local(&identities, hosts, "auberge", "next", tmp.path()).unwrap();

        assert_eq!(updated[0].name, "next");
        assert_eq!(updated[0].ssh_key.as_deref(), Some("/etc/keys/ansible"));
    }

    #[test]
    fn rename_local_rerun_after_partial_failure_converges() {
        let tmp = tempfile::tempdir().unwrap();
        let identities = tmp.path().join(".ssh/identities");
        std::fs::create_dir_all(identities.join("vieille-auberge")).unwrap();
        std::fs::write(identities.join("vieille-auberge/ansible"), b"key").unwrap();

        let hosts = vec![rename_fixture_host("auberge", None)];

        preflight_identities(&identities, "auberge", "vieille-auberge").unwrap();
        let updated =
            rename_local(&identities, hosts, "auberge", "vieille-auberge", tmp.path()).unwrap();

        assert_eq!(updated[0].name, "vieille-auberge");
        assert!(
            identities.join("vieille-auberge/ansible").exists(),
            "already-moved key dir survives the rerun"
        );
    }
}
