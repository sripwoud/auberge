use crate::hosts::HostManager;
use crate::models::inventory::{Host, HostVars, Inventory, RawInventory};
use crate::models::playbook::Playbook;
use crate::playbooks::PlaybookManager;
use eyre::{Result, WrapErr};
use minijinja::Environment;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn find_project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let mut current = cwd.as_path();
    loop {
        if current.join("ansible/inventory.yml").exists() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return cwd,
        }
    }
}

pub fn load_inventory(inventory_path: Option<&Path>) -> Result<Inventory> {
    let path = match inventory_path {
        Some(p) => p.to_path_buf(),
        None => find_project_root().join("ansible/inventory.yml"),
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
    let vars = HostVars {
        ansible_host: xdg_host.address,
        ansible_port: xdg_host.port,
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

pub fn discover_hosts_with_ips(inventory_path: Option<&Path>) -> Result<HashMap<String, String>> {
    let hosts = get_hosts(None, inventory_path)?;

    Ok(hosts
        .into_iter()
        .map(|h| (h.name, h.vars.ansible_host))
        .collect())
}

pub fn get_playbooks(playbooks_path: Option<&Path>) -> Result<Vec<Playbook>> {
    let path = match playbooks_path {
        Some(p) => p.to_path_buf(),
        None => PlaybookManager::get_playbooks_dir()?,
    };

    if !path.exists() {
        eyre::bail!("Playbooks directory not found: {}", path.display());
    }

    let mut playbooks: Vec<Playbook> = std::fs::read_dir(&path)
        .wrap_err_with(|| format!("Failed to read {}", path.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .map(|entry| Playbook::from_path(entry.path()))
        .collect();

    playbooks.sort_by(|a, b| a.name.cmp(&b.name));

    if playbooks.is_empty() {
        eyre::bail!("No playbooks found in: {}", path.display());
    }

    Ok(playbooks)
}
