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
use serde_json::Value;
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
    id: u64,
    name: String,
    created_at: Option<ProtoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtoTimestamp {
    seconds: i64,
    #[serde(default)]
    nanos: i32,
}

impl ProtoTimestamp {
    fn to_rfc3339(&self) -> String {
        chrono::DateTime::from_timestamp(self.seconds, self.nanos as u32)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("{}s", self.seconds))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct HeadscaleNode {
    id: u64,
    #[serde(rename = "givenName")]
    given_name: String,
    #[serde(rename = "ipAddresses", default)]
    ip_addresses: Vec<String>,
    user: HeadscaleNodeUser,
    #[serde(rename = "lastSeen")]
    last_seen: Option<ProtoTimestamp>,
    #[serde(default)]
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
            id: u.id.to_string(),
            name: u.name.clone(),
            created_at: u
                .created_at
                .as_ref()
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
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
            id: n.id.to_string(),
            name: n.given_name.clone(),
            user: n.user.name.clone(),
            ips: n.ip_addresses.join(", "),
            online: if n.online {
                "yes".to_string()
            } else {
                "no".to_string()
            },
            last_seen: n
                .last_seen
                .as_ref()
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
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

fn strip_ssh_banner(output: &str) -> &str {
    let trimmed = output.trim();
    if let Some(pos) = trimmed.rfind("****") {
        let after_banner = &trimmed[pos..];
        if let Some(newline) = after_banner.find('\n') {
            return after_banner[newline..].trim();
        }
    }
    trimmed
}

fn run_headscale_cmd(session: &SshSession, args: &str) -> Result<String> {
    let cmd = format!("sudo headscale {}", args);
    let out = session.run(&cmd)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let cleaned = strip_ssh_banner(&stderr);
        if cleaned.is_empty() {
            eyre::bail!("headscale {} failed", args);
        } else {
            eyre::bail!("headscale {} failed: {}", args, cleaned);
        }
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(strip_ssh_banner(&stdout).to_string())
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
            "preauthkeys create --user {} --expiration {} -o json",
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
    let parsed: Value =
        serde_json::from_str(raw.trim()).wrap_err("Failed to parse headscale nodes list")?;
    let nodes: Vec<HeadscaleNode> = if parsed.is_null() {
        Vec::new()
    } else {
        serde_json::from_value(parsed).wrap_err("Failed to parse headscale nodes list")?
    };

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

    run_headscale_cmd(
        &session,
        &format!("users destroy --name {} --force", username),
    )?;
    output::success(&format!("User '{}' removed", username));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_users_list_json() {
        let json = r#"[
            {"id": 1, "name": "alice", "created_at": {"seconds": 1735689600, "nanos": 0}},
            {"id": 2, "name": "bob", "created_at": {"seconds": 1738368000, "nanos": 0}}
        ]"#;
        let users: Vec<HeadscaleUser> = serde_json::from_str(json).unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "alice");
        assert_eq!(users[1].id, 2);
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
            "id": 1,
            "givenName": "phone",
            "ipAddresses": ["100.64.0.1", "fd7a:115c:a1e0::1"],
            "user": {"name": "alice"},
            "lastSeen": {"seconds": 1712919600, "nanos": 0},
            "online": true
        }]"#;
        let nodes: Vec<HeadscaleNode> = serde_json::from_str(json).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].given_name, "phone");
        assert_eq!(nodes[0].ip_addresses.len(), 2);
        assert!(nodes[0].online);
    }

    #[test]
    fn parse_nodes_null_is_empty() {
        let parsed: Value = serde_json::from_str("null").unwrap();
        assert!(parsed.is_null());
    }

    #[test]
    fn parse_preauthkey_json() {
        let json = r#"{
            "user": "alice",
            "id": "1",
            "key": "abcdef123456",
            "expiration": {"seconds": 1776023355, "nanos": 0},
            "created_at": {"seconds": 1776019755, "nanos": 0}
        }"#;
        let key: HeadscalePreAuthKey = serde_json::from_str(json).unwrap();
        assert_eq!(key.key, "abcdef123456");
    }

    #[test]
    fn node_display_joins_ips() {
        let node = HeadscaleNode {
            id: 1,
            given_name: "test".to_string(),
            ip_addresses: vec!["100.64.0.1".to_string(), "fd7a:115c:a1e0::1".to_string()],
            user: HeadscaleNodeUser {
                name: "alice".to_string(),
            },
            last_seen: Some(ProtoTimestamp {
                seconds: 1735689600,
                nanos: 0,
            }),
            online: true,
        };
        let display = NodeDisplay::from(&node);
        assert_eq!(display.ips, "100.64.0.1, fd7a:115c:a1e0::1");
        assert_eq!(display.online, "yes");
    }

    #[test]
    fn node_display_offline_shows_no() {
        let node = HeadscaleNode {
            id: 1,
            given_name: "test".to_string(),
            ip_addresses: vec![],
            user: HeadscaleNodeUser {
                name: "alice".to_string(),
            },
            last_seen: None,
            online: false,
        };
        let display = NodeDisplay::from(&node);
        assert_eq!(display.online, "no");
        assert_eq!(display.last_seen, "");
    }

    #[test]
    fn user_display_from_headscale_user() {
        let user = HeadscaleUser {
            id: 42,
            name: "carol".to_string(),
            created_at: Some(ProtoTimestamp {
                seconds: 1710504000,
                nanos: 0,
            }),
        };
        let display = UserDisplay::from(&user);
        assert_eq!(display.id, "42");
        assert_eq!(display.name, "carol");
        assert!(display.created_at.contains("2024-03-15"));
    }

    #[test]
    fn proto_timestamp_to_rfc3339() {
        let ts = ProtoTimestamp {
            seconds: 1735689600,
            nanos: 0,
        };
        assert_eq!(ts.to_rfc3339(), "2025-01-01 00:00:00 UTC");
    }

    #[test]
    fn strip_ssh_banner_removes_banner() {
        let output = "************\n* AUTHORIZED *\n************\n{\"key\": \"abc\"}";
        let stripped = strip_ssh_banner(output);
        assert_eq!(stripped, "{\"key\": \"abc\"}");
    }

    #[test]
    fn strip_ssh_banner_no_banner() {
        let output = "{\"key\": \"abc\"}";
        let stripped = strip_ssh_banner(output);
        assert_eq!(stripped, "{\"key\": \"abc\"}");
    }
}
