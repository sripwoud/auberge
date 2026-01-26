use crate::hosts::{Host, HostManager};
use crate::output;
use crate::selector::select_item;
use clap::Subcommand;
use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use eyre::Result;
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
    #[command(about = "Add a new host")]
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
    #[command(about = "List all hosts")]
    List {
        #[arg(short, long, help = "Filter by tags (comma-separated)")]
        tags: Option<String>,
        #[arg(short, long, help = "Output format: table, json, yaml")]
        output: Option<String>,
    },
    #[command(about = "Remove a host")]
    Remove {
        #[arg(help = "Host name")]
        name: String,
        #[arg(short, long, help = "Skip confirmation")]
        yes: bool,
    },
    #[command(about = "Show host details")]
    Show {
        #[arg(help = "Host name")]
        name: String,
        #[arg(short, long, help = "Output format: yaml, json")]
        output: Option<String>,
    },
    #[command(about = "Edit a host")]
    Edit {
        #[arg(help = "Host name")]
        name: String,
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

pub fn run_host_remove(name: String, yes: bool) -> Result<()> {
    let is_tty = HostManager::is_tty();

    if !yes && is_tty {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Remove host '{}'?", name))
            .default(false)
            .interact()?;

        if !confirm {
            println!("Cancelled.");
            return Ok(());
        }
    }

    HostManager::remove_host(&name)?;
    output::success(&format!("Host '{}' removed", name));

    Ok(())
}

pub fn run_host_show(name: String, output: Option<String>) -> Result<()> {
    let host = HostManager::get_host(&name)?;

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

pub fn run_host_edit(name: String) -> Result<()> {
    let host = HostManager::get_host(&name)?;

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
    };

    HostManager::update_host(&name, updated_host)?;
    output::success(&format!("Host '{}' updated", name));

    Ok(())
}
