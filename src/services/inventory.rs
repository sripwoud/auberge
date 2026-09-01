use crate::ansible_assets::AnsibleAssets;
use crate::hosts::HostManager;
use eyre::{Result, WrapErr};
use minijinja::Environment;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn deserialize_port<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrU16 {
        String(String),
        U16(u16),
    }

    match StringOrU16::deserialize(deserializer)? {
        StringOrU16::String(s) => s.parse::<u16>().map_err(serde::de::Error::custom),
        StringOrU16::U16(n) => Ok(n),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostVars {
    pub ansible_host: String,
    #[serde(default = "default_port", deserialize_with = "deserialize_port")]
    pub ansible_port: u16,
    #[serde(default = "default_bootstrap_user")]
    pub bootstrap_user: String,
    #[allow(dead_code)]
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

fn default_port() -> u16 {
    22
}

fn default_bootstrap_user() -> String {
    "root".to_string()
}

#[derive(Debug, Clone)]
pub struct Host {
    pub name: String,
    pub vars: HostVars,
    pub groups: Vec<String>,
}

impl Host {
    /// This Host as the SshSession seam names one. The two representations
    /// carry the same three facts under different names, and only the Inventory
    /// side is reachable from a command that resolved its target from
    /// `ansible/inventory.yml`.
    ///
    /// `user` is a parameter rather than `bootstrap_user`: a command may be
    /// connecting as someone other than the user the Inventory would bootstrap
    /// as, and `ssh add-key` is exactly that case.
    pub fn ssh_target(&self, user: &str) -> crate::hosts::Host {
        crate::hosts::Host {
            name: self.name.clone(),
            address: self.vars.ansible_host.clone(),
            user: user.to_string(),
            port: self.vars.ansible_port,
            ssh_key: None,
            tags: self.groups.clone(),
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
            tailnet_tag: None,
        }
    }

    /// This Host's Route — the `ansible_host`/`ansible_port`/`bootstrap_user`
    /// triple ansible already connects with. Companion to [`Host::ssh_target`],
    /// which builds the same shape for an operator-chosen user rather than the
    /// bootstrap one; the two exist because the caller's user differs by
    /// consumer, not because the underlying facts do.
    ///
    /// No identity key: nothing that constructs an `InventoryHost` from this
    /// opens one of its own (#780) — ansible resolves its own connection.
    pub fn route(&self) -> crate::services::route::Route {
        crate::services::route::Route {
            address: self.vars.ansible_host.clone(),
            port: self.vars.ansible_port,
            user: self.vars.bootstrap_user.clone(),
            key_path: None,
            alias: self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct InventoryGroup {
    #[serde(default)]
    pub hosts: HashMap<String, HostVars>,
    #[serde(default)]
    pub children: HashMap<String, Option<()>>,
    #[allow(dead_code)]
    #[serde(default)]
    pub vars: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AllSection {
    #[serde(default)]
    pub children: HashMap<String, InventoryGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawInventory {
    pub all: AllSection,
}

#[derive(Debug, Clone)]
pub struct Inventory {
    pub groups: HashMap<String, InventoryGroup>,
}

impl Inventory {
    pub fn from_raw(raw: RawInventory) -> Self {
        Self {
            groups: raw.all.children,
        }
    }

    pub fn get_hosts(&self, group: Option<&str>) -> Vec<Host> {
        let mut hosts = Vec::new();
        let mut seen = std::collections::HashSet::new();

        fn collect_from_group(
            inventory: &Inventory,
            group_name: &str,
            inherited_groups: &[String],
            hosts: &mut Vec<Host>,
            seen: &mut std::collections::HashSet<String>,
        ) {
            let Some(grp) = inventory.groups.get(group_name) else {
                return;
            };

            let mut current_groups: Vec<String> = inherited_groups.to_vec();
            current_groups.push(group_name.to_string());

            for child_name in grp.children.keys() {
                collect_from_group(inventory, child_name, &current_groups, hosts, seen);
            }

            for (host_name, host_vars) in &grp.hosts {
                if !seen.contains(host_name) {
                    seen.insert(host_name.clone());
                    hosts.push(Host {
                        name: host_name.clone(),
                        vars: host_vars.clone(),
                        groups: current_groups.clone(),
                    });
                }
            }
        }

        match group {
            Some(g) => collect_from_group(self, g, &[], &mut hosts, &mut seen),
            None => {
                for group_name in self.groups.keys() {
                    collect_from_group(self, group_name, &[], &mut hosts, &mut seen);
                }
            }
        }

        hosts
    }

    pub fn get_host(&self, name: &str) -> Option<Host> {
        self.get_hosts(None).into_iter().find(|h| h.name == name)
    }
}

pub fn load_inventory(inventory_path: Option<&Path>) -> Result<Inventory> {
    let path = match inventory_path {
        Some(p) => p.to_path_buf(),
        None => {
            let assets = AnsibleAssets::prepare()?;
            assets.ansible_dir().join("inventory.yml")
        }
    };

    let content = std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("Failed to read {}", path.display()))?;

    // Render Jinja2 templates with environment variables
    let mut env = Environment::new();
    env.add_function(
        "lookup",
        |kind: String, name: String| -> Result<String, minijinja::Error> {
            if kind == "env" {
                std::env::var(&name).map_err(|_| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::UndefinedError,
                        format!("Environment variable {} not found", name),
                    )
                })
            } else {
                Err(minijinja::Error::new(
                    minijinja::ErrorKind::UndefinedError,
                    format!("Unsupported lookup type: {}", kind),
                ))
            }
        },
    );

    let rendered = env
        .render_str(&content, HashMap::<String, String>::new())
        .wrap_err("Failed to render inventory template")?;

    let raw: RawInventory = serde_yaml::from_str(&rendered)
        .wrap_err_with(|| format!("Failed to parse {}", path.display()))?;

    Ok(Inventory::from_raw(raw))
}

fn convert_xdg_host_to_inventory_host(xdg_host: crate::hosts::Host) -> Host {
    let route = crate::services::route::resolve(&xdg_host, None);
    let vars = HostVars {
        ansible_host: route.address,
        ansible_port: route.port,
        bootstrap_user: xdg_host.user.clone(),
        extra: HashMap::new(),
    };

    Host {
        name: xdg_host.name,
        vars,
        groups: xdg_host.tags,
    }
}

fn try_load_xdg_hosts() -> Result<Option<Vec<Host>>> {
    let xdg_hosts = HostManager::load_hosts()?;

    if xdg_hosts.is_empty() {
        return Ok(None);
    }

    let inventory_hosts: Vec<Host> = xdg_hosts
        .into_iter()
        .map(convert_xdg_host_to_inventory_host)
        .collect();

    Ok(Some(inventory_hosts))
}

pub fn get_hosts(group: Option<&str>, inventory_path: Option<&Path>) -> Result<Vec<Host>> {
    if inventory_path.is_none()
        && let Some(hosts) = try_load_xdg_hosts()?
    {
        if let Some(g) = group {
            return Ok(hosts
                .into_iter()
                .filter(|h| h.groups.contains(&g.to_string()))
                .collect());
        }
        return Ok(hosts);
    }

    let inventory = load_inventory(inventory_path)?;
    Ok(inventory.get_hosts(group))
}

pub fn get_host(name: &str, inventory_path: Option<&Path>) -> Result<Host> {
    if inventory_path.is_none()
        && let Some(hosts) = try_load_xdg_hosts()?
        && let Some(host) = hosts.into_iter().find(|h| h.name == name)
    {
        return Ok(host);
    }

    let inventory = load_inventory(inventory_path)?;
    inventory
        .get_host(name)
        .ok_or_else(|| eyre::eyre!("Host not found: {}", name))
}

/// fail2ban must never ban an Inventory peer: cross-host restores and
/// jump-host recovery source from peer public IPs, and banning one turns a
/// retry storm into a full lockout (#582). Sorted so the rendered jail.local
/// is stable across deploys; comma-separated because a space-separated
/// `-e key=value` would be split into bogus extra pairs by ansible's parse_kv.
fn hosts_ignoreip_value(hosts: &[Host]) -> String {
    let ips: std::collections::BTreeSet<&str> =
        hosts.iter().map(|h| h.vars.ansible_host.as_str()).collect();
    ips.into_iter().collect::<Vec<_>>().join(",")
}

pub fn hosts_ignoreip_var() -> Result<(String, String)> {
    Ok((
        "fail2ban_ignoreip_hosts".to_string(),
        hosts_ignoreip_value(&get_hosts(None, None)?),
    ))
}

pub fn discover_hosts_with_ips(inventory_path: Option<&Path>) -> Result<HashMap<String, String>> {
    let hosts = get_hosts(None, inventory_path)?;

    Ok(hosts
        .into_iter()
        .map(|h| (h.name, h.vars.ansible_host))
        .collect())
}

pub fn select_or_arg(arg: Option<String>, argument: &str) -> Result<Host> {
    match arg {
        Some(name) => get_host(&name, None),
        None => crate::prompt::select_item(
            &get_hosts(None, None)?,
            |h: &Host| {
                format!(
                    "{} ({}:{})",
                    h.name, h.vars.ansible_host, h.vars.ansible_port
                )
            },
            crate::hosts::host_choice(argument),
        ),
    }
}

pub fn get_playbooks(playbooks_path: Option<&Path>) -> Result<Vec<PathBuf>> {
    let path = match playbooks_path {
        Some(p) => p.to_path_buf(),
        None => AnsibleAssets::prepare()?.playbooks_dir(),
    };

    if !path.exists() {
        eyre::bail!("Playbooks directory not found: {}", path.display());
    }

    let mut playbooks: Vec<PathBuf> = std::fs::read_dir(&path)
        .wrap_err_with(|| format!("Failed to read {}", path.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();
            let is_yaml = path
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml");
            let is_meta = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| stem.ends_with(".meta"));
            is_yaml && !is_meta
        })
        .filter_map(|entry| std::fs::canonicalize(entry.path()).ok())
        .collect();

    playbooks.sort_by(|a, b| a.file_stem().cmp(&b.file_stem()));

    if playbooks.is_empty() {
        eyre::bail!("No playbooks found in: {}", path.display());
    }

    Ok(playbooks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ssh_target_carries_address_port_and_groups_across() {
        let mut inventory_host = host("auberge", "203.0.113.7");
        inventory_host.vars.ansible_port = 2222;
        inventory_host.groups = vec!["vps".to_string()];

        let target = inventory_host.ssh_target("ansible");
        assert_eq!(target.name, "auberge");
        assert_eq!(target.address, "203.0.113.7");
        assert_eq!(target.port, 2222);
        assert_eq!(target.tags, vec!["vps".to_string()]);
    }

    #[test]
    fn ssh_target_takes_the_user_it_is_given_not_the_bootstrap_user() {
        let inventory_host = host("auberge", "203.0.113.7");
        assert_eq!(inventory_host.vars.bootstrap_user, "root");
        assert_eq!(inventory_host.ssh_target("sripwoud").user, "sripwoud");
    }

    fn host(name: &str, address: &str) -> Host {
        Host {
            name: name.to_string(),
            vars: HostVars {
                ansible_host: address.to_string(),
                ansible_port: 22,
                bootstrap_user: "root".to_string(),
                extra: HashMap::new(),
            },
            groups: vec![],
        }
    }

    #[test]
    fn test_hosts_ignoreip_value_sorts_and_comma_joins_addresses() {
        let hosts = [host("b", "203.0.113.9"), host("a", "198.51.100.7")];
        assert_eq!(hosts_ignoreip_value(&hosts), "198.51.100.7,203.0.113.9");
    }

    #[test]
    fn test_hosts_ignoreip_value_dedupes_addresses() {
        let hosts = [host("a", "203.0.113.9"), host("b", "203.0.113.9")];
        assert_eq!(hosts_ignoreip_value(&hosts), "203.0.113.9");
    }

    #[test]
    fn test_hosts_ignoreip_value_empty_inventory_yields_empty_string() {
        assert_eq!(hosts_ignoreip_value(&[]), "");
    }

    #[test]
    fn test_get_playbooks_excludes_meta_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["hermes.yml", "hermes.meta.yml", "apps.yml", "apps.meta.yml"] {
            fs::write(dir.path().join(file), "---\n").unwrap();
        }

        let playbooks = get_playbooks(Some(dir.path())).unwrap();
        let stems: Vec<String> = playbooks
            .iter()
            .map(|p| p.file_stem().and_then(|s| s.to_str()).unwrap().to_string())
            .collect();

        assert_eq!(stems, vec!["apps", "hermes"]);
    }
}
