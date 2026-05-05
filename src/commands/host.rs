use crate::hosts::{Host, HostManager};
use crate::output;
use crate::prompt::{confirm, select_item};
use crate::ssh_session::SshSession;
use clap::Subcommand;
use dialoguer::{Input, theme::ColorfulTheme};
use eyre::Result;
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
}

impl From<&Host> for HostDisplay {
    fn from(host: &Host) -> Self {
        Self {
            name: host.name.clone(),
            address: host.address.clone(),
            user: host.user.clone(),
            port: host.port,
            tags: host.tags.join(", "),
        }
    }
}

#[derive(Subcommand)]
pub enum HostCommands {
    #[command(alias = "a", about = "Add a new host")]
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
        #[arg(long, help = "Disable interactive prompts")]
        no_input: bool,
    },
    #[command(alias = "l", about = "List all hosts")]
    List {
        #[arg(short, long, help = "Filter by tags (comma-separated)")]
        tags: Option<String>,
        #[arg(short, long, help = "Output format: table, json, yaml")]
        output: Option<String>,
    },
    #[command(alias = "rm", about = "Remove a host")]
    Remove {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
        #[arg(short, long, help = "Skip confirmation")]
        yes: bool,
    },
    #[command(alias = "s", about = "Show host details")]
    Show {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
        #[arg(short, long, help = "Output format: yaml, json")]
        output: Option<String>,
    },
    #[command(alias = "e", about = "Edit a host")]
    Edit {
        #[arg(help = "Host name (omit to be prompted)")]
        name: Option<String>,
    },
    #[command(
        alias = "dti",
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

        select_item(
            &options,
            |h: &crate::ssh_config::SshConfigHost| {
                if h.hostname.is_none() {
                    "Enter manually".to_string()
                } else {
                    let addr = h.hostname.as_ref().unwrap();
                    let port = h.port.unwrap_or(22);
                    format!("{} ({}:{})", h.name, addr, port)
                }
            },
            "Import from SSH config or enter manually?",
        )?
        .and_then(|h| if h.hostname.is_some() { Some(h) } else { None })
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
    };

    HostManager::add_host(host)?;

    let config_path = HostManager::config_path()?;
    output::success(&format!(
        "Host '{}' added to {}",
        name,
        config_path.display()
    ));

    Ok(())
}

pub fn run_host_list(tags: Option<String>, output: Option<String>) -> Result<()> {
    let filter_tags = tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

    let hosts = HostManager::list_hosts_filtered(filter_tags)?;

    if hosts.is_empty() {
        output::info("No hosts configured yet");
        println!("\nAdd a host with:");
        println!("  auberge host add <name> <address>");
        return Ok(());
    }

    match output.as_deref() {
        Some("json") => {
            println!("{}", serde_json::to_string_pretty(&hosts)?);
        }
        Some("yaml") => {
            println!("{}", serde_yaml::to_string(&hosts)?);
        }
        _ => {
            let display_hosts: Vec<HostDisplay> = hosts.iter().map(HostDisplay::from).collect();
            output::print_table(&display_hosts);
        }
    }

    Ok(())
}

pub fn run_host_remove(name: Option<String>, yes: bool) -> Result<()> {
    let host = crate::hosts::select_or_arg(name)?;
    if !confirm(&format!("Remove host '{}'?", host.name), yes) {
        println!("Cancelled.");
        return Ok(());
    }

    HostManager::remove_host(&host.name)?;
    output::success(&format!("Host '{}' removed", host.name));

    Ok(())
}

pub fn run_host_show(name: Option<String>, output: Option<String>) -> Result<()> {
    let host = crate::hosts::select_or_arg(name)?;

    match output.as_deref() {
        Some("json") => {
            println!("{}", serde_json::to_string_pretty(&host)?);
        }
        _ => {
            println!("{}", serde_yaml::to_string(&host)?);
        }
    }

    Ok(())
}

pub fn run_host_detect_tailscale_ip(name_arg: Option<String>) -> Result<()> {
    let host = crate::hosts::select_or_arg(name_arg)?;
    let ssh_key = resolve_ssh_key(&host)?;
    let session = SshSession::new(&host, &ssh_key);

    output::info(&format!(
        "Querying Tailscale IPv4 on {}@{}…",
        host.user, host.address
    ));

    let out = session.run("tailscale ip -4")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            eyre::bail!("`tailscale ip -4` failed on {}", host.name);
        }
        eyre::bail!("`tailscale ip -4` failed on {}: {}", host.name, stderr);
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let detected = parse_tailscale_cgnat_ipv4(&stdout).ok_or_else(|| {
        eyre::eyre!(
            "No Tailscale CGNAT IPv4 found in `tailscale ip -4` output for {}: {:?}",
            host.name,
            stdout.trim()
        )
    })?;

    let mut updated = host.clone();
    updated.tailscale_ip = Some(detected.clone());
    HostManager::update_host(&host.name, updated)?;

    output::success(&format!(
        "Cached tailscale_ip={} for host '{}'",
        detected, host.name
    ));
    Ok(())
}

fn resolve_ssh_key(host: &Host) -> Result<PathBuf> {
    let key = match host.ssh_key.as_ref() {
        Some(p) => PathBuf::from(shellexpand::tilde(p).into_owned()),
        None => dirs::home_dir()
            .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
            .join(format!(".ssh/identities/{}_{}", host.user, host.name)),
    };
    if !key.exists() {
        eyre::bail!(
            "SSH key not found: {}. Run 'auberge ssh keygen --host {}' first.",
            key.display(),
            host.name
        );
    }
    Ok(key)
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

pub fn run_host_edit(name: Option<String>) -> Result<()> {
    let host = crate::hosts::select_or_arg(name)?;
    let name = host.name.clone();

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

    let updated_host = Host {
        name: name.clone(),
        address,
        user,
        port,
        ssh_key: host.ssh_key,
        tags: tags_vec,
        description: if description.is_empty() {
            None
        } else {
            Some(description)
        },
        python_interpreter: host.python_interpreter,
        become_method: host.become_method,
        tailscale_ip: host.tailscale_ip,
    };

    HostManager::update_host(&name, updated_host)?;
    output::success(&format!("Host '{}' updated", name));

    Ok(())
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

    // host show/edit/remove: with None on a non-TTY, select_or_arg fails before
    // prompting; with Some(unknown_name), the host lookup fails. Both must error.

    #[test]
    fn host_commands_error_on_none_or_unknown() {
        let unknown = || Some("__nonexistent_host__".to_string());

        assert!(run_host_show(None, None).is_err());
        assert!(run_host_show(unknown(), None).is_err());

        assert!(run_host_remove(None, true).is_err());
        assert!(run_host_remove(unknown(), true).is_err());

        assert!(run_host_edit(None).is_err());
        assert!(run_host_edit(unknown()).is_err());
    }
}
