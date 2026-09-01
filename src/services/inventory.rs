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
    /// The Inventory's own declaration. `ansible_host` here is the Host's
    /// **public** address and stays that way under #787's routing policy: it
    /// is what `dns set-all` publishes as an A record and what a deploy's
    /// public DNS check expects to see. Where the CLI *connects* is
    /// [`Host::connect_address`] — two questions one field used to answer.
    pub vars: HostVars,
    pub groups: Vec<String>,
    /// Where this CLI connects, already resolved through
    /// `services::route::resolve` — so #787's `prefer_tailnet` and `--via`
    /// reach ansible's own `ansible_host` var, not just ssh.
    ///
    /// Resolved once, at the roster→Inventory conversion, rather than
    /// re-derived by [`Host::route`]: a second implementation of the policy
    /// is a second thing to keep in step. Equal to `vars.ansible_host` for a
    /// Host read from `ansible/inventory.yml`, which declares no tailnet
    /// facts and therefore no policy to apply.
    pub connect_address: String,
}

impl Host {
    /// A Host straight from `ansible/inventory.yml`: no roster entry, so no
    /// routing policy, so the declared address is also the route.
    fn from_inventory(name: String, vars: HostVars, groups: Vec<String>) -> Self {
        let connect_address = vars.ansible_host.clone();
        Self {
            name,
            vars,
            groups,
            connect_address,
        }
    }
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
            address: self.connect_address.clone(),
            user: user.to_string(),
            port: self.vars.ansible_port,
            ssh_key: None,
            tags: self.groups.clone(),
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
            tailnet_tag: None,
            prefer_tailnet: false,
            unknown: toml::Table::new(),
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
            address: self.connect_address.clone(),
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
                    hosts.push(Host::from_inventory(
                        host_name.clone(),
                        host_vars.clone(),
                        current_groups.clone(),
                    ));
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

/// The roster entry as the Inventory sees it, with both addresses kept
/// apart: `ansible_host` is the declared public one, `connect_address` the
/// one #787's policy resolved. Folding them back together publishes a CGNAT
/// address as a public A record.
fn convert_xdg_host_to_inventory_host(xdg_host: crate::hosts::Host) -> Result<Host> {
    let route = crate::services::route::resolve(&xdg_host, None)?;
    let vars = HostVars {
        ansible_host: crate::services::route::public_address(&xdg_host),
        ansible_port: route.port,
        bootstrap_user: xdg_host.user.clone(),
        extra: HashMap::new(),
    };

    Ok(Host {
        name: xdg_host.name,
        vars,
        groups: xdg_host.tags,
        connect_address: route.address,
    })
}

fn try_load_xdg_hosts() -> Result<Option<Vec<Host>>> {
    let xdg_hosts = HostManager::load_hosts()?;

    if xdg_hosts.is_empty() {
        return Ok(None);
    }

    let inventory_hosts: Vec<Host> = xdg_hosts
        .into_iter()
        .map(convert_xdg_host_to_inventory_host)
        .collect::<Result<_>>()?;

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
/// Both addresses of every peer, not just the one currently in use (#787): a
/// Host that routes over the tailnet arrives at a peer's sshd from its
/// tailnet address, while `--via public` recovery arrives from its public
/// one, and the recovery route must not be the bannable one.
fn hosts_ignoreip_value(hosts: &[Host], peers: &[String]) -> String {
    let ips: std::collections::BTreeSet<&str> = hosts
        .iter()
        .map(|h| h.vars.ansible_host.as_str())
        .chain(peers.iter().map(String::as_str))
        .collect();
    ips.into_iter().collect::<Vec<_>>().join(",")
}

pub fn hosts_ignoreip_var() -> Result<(String, String)> {
    let peers = crate::services::route::peer_addresses(&HostManager::load_hosts()?);
    Ok((
        "fail2ban_ignoreip_hosts".to_string(),
        hosts_ignoreip_value(&get_hosts(None, None)?, &peers),
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
        Host::from_inventory(
            name.to_string(),
            HostVars {
                ansible_host: address.to_string(),
                ansible_port: 22,
                bootstrap_user: "root".to_string(),
                extra: HashMap::new(),
            },
            vec![],
        )
    }

    #[test]
    fn test_hosts_ignoreip_value_sorts_and_comma_joins_addresses() {
        let hosts = [host("b", "203.0.113.9"), host("a", "198.51.100.7")];
        assert_eq!(
            hosts_ignoreip_value(&hosts, &[]),
            "198.51.100.7,203.0.113.9"
        );
    }

    #[test]
    fn test_hosts_ignoreip_value_dedupes_addresses() {
        let hosts = [host("a", "203.0.113.9"), host("b", "203.0.113.9")];
        assert_eq!(hosts_ignoreip_value(&hosts, &[]), "203.0.113.9");
    }

    #[test]
    fn test_hosts_ignoreip_value_empty_inventory_yields_empty_string() {
        assert_eq!(hosts_ignoreip_value(&[], &[]), "");
    }

    /// A Host routing over the tailnet arrives at its peers from
    /// `100.64.0.1`; the same Host under `--via public` arrives from
    /// `203.0.113.10`. fail2ban has to ignore both, or the recovery route is
    /// the bannable one (#787).
    #[test]
    fn hosts_ignoreip_value_keeps_a_peers_public_address_alongside_its_tailnet_one() {
        let hosts = [host("auberge", "203.0.113.10")];
        let peers = crate::services::route::peer_addresses(&[crate::hosts::Host::fixture(
            "auberge",
            Some("100.64.0.1"),
        )
        .preferring_tailnet()]);

        assert_eq!(
            hosts_ignoreip_value(&hosts, &peers),
            "100.64.0.1,203.0.113.10"
        );
    }

    /// The Inventory's `ansible_host` must stay public even for a Host that
    /// routes over the tailnet: `dns set-all` reads it as the A record's
    /// value, and a deploy's public DNS check as the address that record must
    /// resolve to.
    #[test]
    fn the_tailnet_policy_moves_the_route_and_leaves_ansible_host_public() {
        let converted = convert_xdg_host_to_inventory_host(
            crate::hosts::Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet(),
        )
        .unwrap();

        assert_eq!(converted.vars.ansible_host, "203.0.113.10");
        assert_eq!(converted.connect_address, "100.64.0.1");
        assert_eq!(converted.route().address, "100.64.0.1");
        assert_eq!(converted.ssh_target("admin").address, "100.64.0.1");
    }

    /// A Host read from `ansible/inventory.yml` carries no policy, so the
    /// declared address is also the route — the conversion above is the only
    /// thing that can move one.
    #[test]
    fn an_inventory_host_routes_to_the_address_it_declares() {
        let host = host("auberge", "203.0.113.7");
        assert_eq!(host.connect_address, host.vars.ansible_host);
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
