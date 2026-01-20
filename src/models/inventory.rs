use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct HostVars {
    pub ansible_host: String,
    #[serde(default = "default_port")]
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
    #[allow(dead_code)]
    pub groups: Vec<String>,
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
