use crate::hosts::{Host, HostManager};
use crate::output;
use crate::selector::select_item;
use crate::ssh_session::SshSession;
use crate::user_config::UserConfig;
use clap::Subcommand;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, Select};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::path::PathBuf;
use tabled::Tabled;

#[derive(Subcommand)]
pub enum HeadscaleCommands {
    #[command(alias = "au", about = "Create a user and generate a pre-auth key")]
    AddUser {
        #[arg(help = "Username to create")]
        name: Option<String>,
        #[arg(
            short,
            long,
            help = "Pre-auth key expiration (e.g. 1h, 24h, 48h, 7d)",
            default_value = "24h"
        )]
        expiration: Option<String>,
        #[arg(long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(alias = "lu", about = "List registered users")]
    ListUsers {
        #[arg(short, long, help = "Output format: table, json")]
        output: Option<String>,
        #[arg(long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(alias = "ln", about = "List connected nodes")]
    ListNodes {
        #[arg(short, long, help = "Output format: table, json")]
        output: Option<String>,
        #[arg(long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(alias = "ru", about = "Remove a user")]
    RemoveUser {
        #[arg(help = "Username to remove")]
        name: Option<String>,
        #[arg(short, long, help = "Skip confirmation prompt")]
        yes: bool,
        #[arg(long, help = "Target host running headscale")]
        host: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeadscaleUser {
    id: String,
    name: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeadscaleNode {
    id: String,
    #[serde(rename = "givenName")]
    given_name: String,
    #[serde(rename = "ipAddresses")]
    ip_addresses: Vec<String>,
    user: HeadscaleNodeUser,
    #[serde(rename = "lastSeen")]
    last_seen: String,
    online: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeadscaleNodeUser {
    name: String,
}

#[derive(Debug, Deserialize)]
struct HeadscalePreAuthKey {
    key: String,
}

#[derive(Tabled)]
struct UserDisplay {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "CREATED")]
    created_at: String,
}

impl From<&HeadscaleUser> for UserDisplay {
    fn from(u: &HeadscaleUser) -> Self {
        Self {
            id: u.id.clone(),
            name: u.name.clone(),
            created_at: u.created_at.clone(),
        }
    }
}

#[derive(Tabled)]
struct NodeDisplay {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "USER")]
    user: String,
    #[tabled(rename = "IPS")]
    ips: String,
    #[tabled(rename = "ONLINE")]
    online: String,
    #[tabled(rename = "LAST SEEN")]
    last_seen: String,
}

impl From<&HeadscaleNode> for NodeDisplay {
    fn from(n: &HeadscaleNode) -> Self {
        Self {
            id: n.id.clone(),
            name: n.given_name.clone(),
            user: n.user.name.clone(),
            ips: n.ip_addresses.join(", "),
            online: if n.online {
                "yes".to_string()
            } else {
                "no".to_string()
            },
            last_seen: n.last_seen.clone(),
        }
    }
}

fn resolve_headscale_host(host_arg: Option<String>) -> Result<(Host, PathBuf)> {
    let host = match host_arg {
        Some(name) => HostManager::get_host(&name)?,
        None => {
            let config = UserConfig::load()?;
            match config.get("hostname") {
                Some(name) if !name.is_empty() => HostManager::get_host(&name)?,
                _ => {
                    let hosts = HostManager::load_hosts()?;
                    if hosts.is_empty() {
                        eyre::bail!(
                            "No hosts configured. Run 'auberge host add' to add a host first"
                        );
                    }
                    select_item(
                        &hosts,
                        |h| format!("{} ({})", h.name, h.address),
                        "Select host",
                    )?
                    .ok_or_else(|| eyre::eyre!("No host selected"))?
                }
            }
        }
    };

    let ssh_key = match &host.ssh_key {
        Some(key) => {
            let path = PathBuf::from(shellexpand::tilde(key).as_ref());
            if !path.exists() {
                eyre::bail!("SSH key not found: {}", path.display());
            }
            path
        }
        None => {
            let path = dirs::home_dir()
                .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
                .join(format!(".ssh/identities/{}_{}", host.user, host.name));
            if !path.exists() {
                eyre::bail!(
                    "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}'",
                    path.display(),
                    host.name,
                    host.user
                );
            }
            path
        }
    };

    Ok((host, ssh_key))
}

fn run_headscale_cmd(session: &SshSession, args: &str) -> Result<String> {
    let cmd = format!("sudo headscale {}", args);
    let out = session.run(&cmd)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        if msg.is_empty() {
            eyre::bail!("headscale {} failed", args);
        } else {
            eyre::bail!("headscale {} failed: {}", args, msg);
        }
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn run_headscale_add_user(
    name: Option<String>,
    expiration: Option<String>,
    host: Option<String>,
) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = SshSession::new(&host_info, &ssh_key);

    let is_tty = std::io::stdin().is_terminal();

    let username = match name {
        Some(n) => n,
        None if is_tty => Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Username")
            .interact_text()?,
        None => eyre::bail!("Username is required (pass as argument or run interactively)"),
    };

    let exp = match expiration {
        Some(e) => e,
        None if is_tty => {
            let options = vec!["1h", "24h", "48h", "7d"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Key expiration")
                .items(&options)
                .default(1)
                .interact()?;
            options[selection].to_string()
        }
        None => "24h".to_string(),
    };

    output::info(&format!("Creating user '{}'...", username));
    run_headscale_cmd(&session, &format!("users create {}", username))?;
    output::success(&format!("User '{}' created", username));

    output::info("Generating pre-auth key...");
    let key_output = run_headscale_cmd(
        &session,
        &format!(
            "preauthkeys create --user {} --expiration {}",
            username, exp
        ),
    )?;

    let key: HeadscalePreAuthKey = serde_json::from_str(key_output.trim())
        .wrap_err("Failed to parse pre-auth key response")?;

    let config = UserConfig::load()?;
    let subdomain = config
        .get("headscale_subdomain")
        .unwrap_or_else(|| "hs".to_string());
    let domain = config
        .get("domain")
        .unwrap_or_else(|| "example.com".to_string());
    let login_server = format!("https://{}.{}", subdomain, domain);

    println!();
    println!("Pre-auth key: {}", key.key);
    println!();
    println!("Share these instructions:");
    println!("─────────────────────────────────────");
    println!("1. Install Tailscale from the App Store");
    println!("2. Long-press the ⋯ menu → \"Use custom coordination server\"");
    println!("3. Enter: {}", login_server);
    println!("4. Use pre-auth key: {}", key.key);
    println!("─────────────────────────────────────");

    output::success("Done");
    Ok(())
}

pub fn run_headscale_list_users(output_fmt: Option<String>, host: Option<String>) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = SshSession::new(&host_info, &ssh_key);

    let raw = run_headscale_cmd(&session, "users list -o json")?;
    let users: Vec<HeadscaleUser> =
        serde_json::from_str(raw.trim()).wrap_err("Failed to parse headscale users list")?;

    match output_fmt.as_deref() {
        Some("json") => {
            println!("{}", serde_json::to_string_pretty(&users)?);
        }
        _ => {
            if users.is_empty() {
                output::info("No users found");
                return Ok(());
            }
            let display: Vec<UserDisplay> = users.iter().map(UserDisplay::from).collect();
            output::print_table(&display);
        }
    }
    Ok(())
}

pub fn run_headscale_list_nodes(output_fmt: Option<String>, host: Option<String>) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = SshSession::new(&host_info, &ssh_key);

    let raw = run_headscale_cmd(&session, "nodes list -o json")?;
    let nodes: Vec<HeadscaleNode> =
        serde_json::from_str(raw.trim()).wrap_err("Failed to parse headscale nodes list")?;

    match output_fmt.as_deref() {
        Some("json") => {
            println!("{}", serde_json::to_string_pretty(&nodes)?);
        }
        _ => {
            if nodes.is_empty() {
                output::info("No nodes found");
                return Ok(());
            }
            let display: Vec<NodeDisplay> = nodes.iter().map(NodeDisplay::from).collect();
            output::print_table(&display);
        }
    }
    Ok(())
}

pub fn run_headscale_remove_user(
    name: Option<String>,
    yes: bool,
    host: Option<String>,
) -> Result<()> {
    if yes && name.is_none() {
        eyre::bail!("--yes requires a username argument");
    }

    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = SshSession::new(&host_info, &ssh_key);

    let is_tty = std::io::stdin().is_terminal();

    let username = match name {
        Some(n) => n,
        None if is_tty => {
            let raw = run_headscale_cmd(&session, "users list -o json")?;
            let users: Vec<HeadscaleUser> =
                serde_json::from_str(raw.trim()).wrap_err("Failed to parse users list")?;
            if users.is_empty() {
                eyre::bail!("No users to remove");
            }
            let selected = select_item(
                &users,
                |u| format!("{} (id: {})", u.name, u.id),
                "Select user to remove",
            )?
            .ok_or_else(|| eyre::eyre!("No user selected"))?;
            selected.name.clone()
        }
        None => eyre::bail!("Username is required (pass as argument or run interactively)"),
    };

    if !yes && is_tty {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Remove user '{}'?", username))
            .default(false)
            .interact()?;
        if !confirm {
            output::info("Cancelled");
            return Ok(());
        }
    }

    run_headscale_cmd(&session, &format!("users destroy {}", username))?;
    output::success(&format!("User '{}' removed", username));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_users_list_json() {
        let json = r#"[
            {"id": "1", "name": "alice", "createdAt": "2026-01-01T00:00:00Z"},
            {"id": "2", "name": "bob", "createdAt": "2026-02-01T00:00:00Z"}
        ]"#;
        let users: Vec<HeadscaleUser> = serde_json::from_str(json).unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "alice");
        assert_eq!(users[1].id, "2");
    }

    #[test]
    fn parse_users_list_json_empty() {
        let json = "[]";
        let users: Vec<HeadscaleUser> = serde_json::from_str(json).unwrap();
        assert!(users.is_empty());
    }

    #[test]
    fn parse_nodes_list_json() {
        let json = r#"[{
            "id": "1",
            "givenName": "phone",
            "ipAddresses": ["100.64.0.1", "fd7a:115c:a1e0::1"],
            "user": {"name": "alice"},
            "lastSeen": "2026-04-12T10:00:00Z",
            "online": true
        }]"#;
        let nodes: Vec<HeadscaleNode> = serde_json::from_str(json).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].given_name, "phone");
        assert_eq!(nodes[0].ip_addresses.len(), 2);
        assert!(nodes[0].online);
    }

    #[test]
    fn parse_nodes_online_field() {
        let online_json = r#"[{
            "id": "1", "givenName": "a", "ipAddresses": [],
            "user": {"name": "x"}, "lastSeen": "", "online": true
        }]"#;
        let offline_json = r#"[{
            "id": "1", "givenName": "a", "ipAddresses": [],
            "user": {"name": "x"}, "lastSeen": "", "online": false
        }]"#;
        let online: Vec<HeadscaleNode> = serde_json::from_str(online_json).unwrap();
        let offline: Vec<HeadscaleNode> = serde_json::from_str(offline_json).unwrap();
        assert!(online[0].online);
        assert!(!offline[0].online);
    }

    #[test]
    fn parse_preauthkey_json() {
        let json = r#"{"key": "abcdef123456"}"#;
        let key: HeadscalePreAuthKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.key, "abcdef123456");
    }

    #[test]
    fn node_display_joins_ips() {
        let node = HeadscaleNode {
            id: "1".to_string(),
            given_name: "test".to_string(),
            ip_addresses: vec!["100.64.0.1".to_string(), "fd7a:115c:a1e0::1".to_string()],
            user: HeadscaleNodeUser {
                name: "alice".to_string(),
            },
            last_seen: "2026-01-01T00:00:00Z".to_string(),
            online: true,
        };
        let display = NodeDisplay::from(&node);
        assert_eq!(display.ips, "100.64.0.1, fd7a:115c:a1e0::1");
        assert_eq!(display.online, "yes");
    }

    #[test]
    fn node_display_offline_shows_no() {
        let node = HeadscaleNode {
            id: "1".to_string(),
            given_name: "test".to_string(),
            ip_addresses: vec![],
            user: HeadscaleNodeUser {
                name: "alice".to_string(),
            },
            last_seen: String::new(),
            online: false,
        };
        let display = NodeDisplay::from(&node);
        assert_eq!(display.online, "no");
    }

    #[test]
    fn user_display_from_headscale_user() {
        let user = HeadscaleUser {
            id: "42".to_string(),
            name: "carol".to_string(),
            created_at: "2026-03-15T12:00:00Z".to_string(),
        };
        let display = UserDisplay::from(&user);
        assert_eq!(display.id, "42");
        assert_eq!(display.name, "carol");
        assert_eq!(display.created_at, "2026-03-15T12:00:00Z");
    }
}
