use crate::models::inventory::{Host, Inventory, RawInventory};
use crate::models::playbook::Playbook;
use eyre::{Result, WrapErr};
use std::path::{Path, PathBuf};

pub fn find_project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let mut current = cwd.as_path();
    loop {
        if current.join("inventory.yml").exists() {
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
        None => find_project_root().join("inventory.yml"),
    };

    let content = std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("Failed to read {}", path.display()))?;

    let raw: RawInventory = serde_yaml::from_str(&content)
        .wrap_err_with(|| format!("Failed to parse {}", path.display()))?;

    Ok(Inventory::from_raw(raw))
}

pub fn get_hosts(group: Option<&str>, inventory_path: Option<&Path>) -> Result<Vec<Host>> {
    let inventory = load_inventory(inventory_path)?;
    Ok(inventory.get_hosts(group))
}

pub fn get_host(name: &str, inventory_path: Option<&Path>) -> Result<Host> {
    let inventory = load_inventory(inventory_path)?;
    inventory
        .get_host(name)
        .ok_or_else(|| eyre::eyre!("Host not found: {}", name))
}

pub fn get_playbooks(playbooks_path: Option<&Path>) -> Result<Vec<Playbook>> {
    let path = match playbooks_path {
        Some(p) => p.to_path_buf(),
        None => find_project_root().join("playbooks"),
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
