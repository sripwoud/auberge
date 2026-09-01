use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub address: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_interpreter: Option<String>,
    #[serde(default = "default_become_method")]
    pub become_method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_ip: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HostsConfig {
    hosts: Vec<Host>,
}

fn default_port() -> u16 {
    22
}

fn default_become_method() -> String {
    "sudo".to_string()
}

/// The Hosts whose config answers a serving gate: the ADR-0051 shape — config
/// alone answers it, and a blank value is no answer — read through ADR-0058's
/// host-scoped view, so a `[hosts.<name>]` override decides for that Host.
///
/// Zero and several answers mean different things per gate, so both come back
/// as they are and the caller says which is a problem.
pub fn serving_hosts<'a>(
    hosts: &'a [Host],
    config: &crate::config::Config,
    gate_key: &str,
) -> Vec<&'a Host> {
    hosts
        .iter()
        .filter(|h| {
            config
                .get_for_host(gate_key, Some(&h.name))
                .is_some_and(|v| !v.trim().is_empty())
        })
        .collect()
}

impl Host {
    /// A roster entry with only the two fields the gate lookups read, so a
    /// unit test elsewhere in the crate does not restate the other eight.
    /// Test-only, like `Config::from_toml_str`.
    #[cfg(test)]
    pub fn fixture(name: &str, tailscale_ip: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: tailscale_ip.map(str::to_string),
        }
    }
}

pub struct HostManager;

impl HostManager {
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = crate::config::Config::config_dir()?;
        Ok(config_dir.join("hosts.toml"))
    }

    pub fn load_hosts() -> Result<Vec<Host>> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(&config_path)
            .wrap_err_with(|| format!("Failed to read hosts config: {}", config_path.display()))?;

        let config: HostsConfig =
            toml::from_str(&contents).wrap_err("Failed to parse hosts.toml")?;

        Ok(config.hosts)
    }

    pub fn save_hosts(hosts: &[Host]) -> Result<()> {
        let config_path = Self::config_path()?;

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).wrap_err_with(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let config = HostsConfig {
            hosts: hosts.to_vec(),
        };

        let contents =
            toml::to_string_pretty(&config).wrap_err("Failed to serialize hosts config")?;

        fs::write(&config_path, contents)
            .wrap_err_with(|| format!("Failed to write hosts config: {}", config_path.display()))?;

        Ok(())
    }

    pub fn add_host(host: Host) -> Result<()> {
        let mut hosts = Self::load_hosts()?;

        if hosts.iter().any(|h| h.name == host.name) {
            eyre::bail!("Host '{}' already exists", host.name);
        }

        hosts.push(host);
        Self::save_hosts(&hosts)?;

        Ok(())
    }

    pub fn remove_host(name: &str) -> Result<()> {
        let mut hosts = Self::load_hosts()?;

        let original_len = hosts.len();
        hosts.retain(|h| h.name != name);

        if hosts.len() == original_len {
            eyre::bail!("Host '{}' not found", name);
        }

        Self::save_hosts(&hosts)?;

        Ok(())
    }

    pub fn get_host(name: &str) -> Result<Host> {
        let hosts = Self::load_hosts()?;

        hosts
            .into_iter()
            .find(|h| h.name == name)
            .ok_or_else(|| eyre::eyre!("Host '{}' not found", name))
    }

    pub fn update_host(name: &str, updated_host: Host) -> Result<()> {
        let mut hosts = Self::load_hosts()?;

        let host = hosts
            .iter_mut()
            .find(|h| h.name == name)
            .ok_or_else(|| eyre::eyre!("Host '{}' not found", name))?;

        *host = updated_host;

        Self::save_hosts(&hosts)?;

        Ok(())
    }

    pub fn list_hosts_filtered(tags: Option<Vec<String>>) -> Result<Vec<Host>> {
        let hosts = Self::load_hosts()?;

        if let Some(filter_tags) = tags {
            Ok(hosts
                .into_iter()
                .filter(|h| filter_tags.iter().any(|tag| h.tags.contains(tag)))
                .collect())
        } else {
            Ok(hosts)
        }
    }

    pub fn is_tty() -> bool {
        std::io::stdin().is_terminal()
    }
}

/// How a command names its host, for the error raised when no picker can be
/// drawn. Commands take the host either as a flag or as a positional argument;
/// naming the wrong one is worse than naming none.
pub const HOST_FLAG: &str = "-H <host>";
pub const HOST_POSITIONAL: &str = "the host name as an argument";

pub fn host_choice(argument: &str) -> crate::prompt::Choice {
    crate::prompt::Choice::new("host")
        .resolved_by(argument)
        .populated_by("auberge host add")
}

pub fn select_or_arg(arg: Option<String>, argument: &str) -> eyre::Result<Host> {
    match arg {
        Some(name) => HostManager::get_host(&name),
        None => crate::prompt::select_item(
            &HostManager::load_hosts()?,
            |h: &Host| format!("{} ({}:{})", h.name, h.address, h.port),
            host_choice(argument),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn names<'a>(hosts: &[&'a Host]) -> Vec<&'a str> {
        hosts.iter().map(|h| h.name.as_str()).collect()
    }

    #[test]
    fn serving_hosts_counts_every_host_a_fleet_wide_answer_reaches() {
        let hosts = [Host::fixture("auberge", None), Host::fixture("ruche", None)];
        let config = Config::from_toml_str(r#"blocky_subdomain = "dns""#).unwrap();
        assert_eq!(
            names(&serving_hosts(&hosts, &config, "blocky_subdomain")),
            vec!["auberge", "ruche"]
        );
    }

    #[test]
    fn serving_hosts_drops_a_host_that_blanked_the_gate() {
        let hosts = [Host::fixture("auberge", None), Host::fixture("ruche", None)];
        let config = Config::from_toml_str(
            r#"
blocky_subdomain = "dns"

[hosts.ruche]
blocky_subdomain = ""
"#,
        )
        .unwrap();
        assert_eq!(
            names(&serving_hosts(&hosts, &config, "blocky_subdomain")),
            vec!["auberge"]
        );
    }

    #[test]
    fn serving_hosts_counts_a_host_that_answers_only_in_its_own_table() {
        let hosts = [Host::fixture("auberge", None), Host::fixture("ruche", None)];
        let config = Config::from_toml_str(
            r#"
[hosts.ruche]
blocky_subdomain = "dns"
"#,
        )
        .unwrap();
        assert_eq!(
            names(&serving_hosts(&hosts, &config, "blocky_subdomain")),
            vec!["ruche"]
        );
    }

    #[test]
    fn serving_hosts_is_empty_when_nothing_answers_the_gate() {
        let hosts = [Host::fixture("auberge", None)];
        let config = Config::from_toml_str(r#"domain = "example.com""#).unwrap();
        assert!(serving_hosts(&hosts, &config, "blocky_subdomain").is_empty());
    }

    #[test]
    fn test_host_serialization() {
        let host = Host {
            name: "test".to_string(),
            address: "192.168.1.1".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec!["production".to_string()],
            description: Some("Test host".to_string()),
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
        };

        let config = HostsConfig { hosts: vec![host] };

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: HostsConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.hosts.len(), 1);
        assert_eq!(parsed.hosts[0].name, "test");
        assert_eq!(parsed.hosts[0].port, 22);
    }

    #[test]
    fn test_default_values() {
        let toml_str = r#"
            [[hosts]]
            name = "minimal"
            address = "1.2.3.4"
            user = "root"
        "#;

        let config: HostsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hosts[0].port, 22);
        assert_eq!(config.hosts[0].become_method, "sudo");
        assert!(config.hosts[0].tags.is_empty());
        assert!(config.hosts[0].tailscale_ip.is_none());
    }

    #[test]
    fn test_tailscale_ip_round_trip() {
        let host = Host {
            name: "vps".to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: Some("100.64.0.5".to_string()),
        };

        let serialized = toml::to_string(&HostsConfig { hosts: vec![host] }).unwrap();
        assert!(serialized.contains("tailscale_ip = \"100.64.0.5\""));

        let parsed: HostsConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.hosts[0].tailscale_ip.as_deref(), Some("100.64.0.5"));
    }

    #[test]
    fn test_tailscale_ip_omitted_when_none() {
        let host = Host {
            name: "vps".to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
        };

        let serialized = toml::to_string(&HostsConfig { hosts: vec![host] }).unwrap();
        assert!(!serialized.contains("tailscale_ip"));
    }
}
