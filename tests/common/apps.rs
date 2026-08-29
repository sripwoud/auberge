//! The App layer over the walk: which App answers for a role, and which
//! units each App declares as its Unit Ownership (`units:` in the Playbook
//! Meta, ADR-0042).
//!
//! Two fences read this layer, which is why it lives here rather than in
//! either of them (#654's lesson, one layer up): `unit_ownership` holds the
//! declarations to the scan in both directions, and `probe_after_restart`'s
//! presence fence takes the declarations as its domain authority — an App
//! with a Serving Unit must probe it, and the Meta is the only inventory
//! that holds the units no task reveals (#720). A copy in each would be the
//! divergence-that-does-not-fail this directory exists to remove.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use auberge::playbook_meta::qualified_unit_name;
use serde_yaml::{Mapping, Value};

use super::units::Scope;
use super::{all_roles, field, meta_files, parse_yaml, playbook_files, playbooks_dir, role_dir};

/// One unit an App owns: its `systemctl` name and the manager it lives in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnedUnit {
    pub app: String,
    pub unit: String,
    pub scope: Scope,
}

impl OwnedUnit {
    pub fn id(&self) -> String {
        let scope = match self.scope {
            Scope::System => "",
            Scope::User => " (user)",
        };
        format!("{}/{}{}", self.app, self.unit, scope)
    }
}

/// The App each role answers as: the role's own name where a Meta of that
/// name exists, otherwise the playbook that runs the role, otherwise the App
/// of a role that depends on it — claude_code_remote deploys as a dependency
/// of the vibecoder role, through vibecoder.yml, so its Meta is vibecoder's.
pub fn app_of(role: &str) -> Option<String> {
    resolve_app(role, &mut BTreeSet::new())
}

fn resolve_app(role: &str, visited: &mut BTreeSet<String>) -> Option<String> {
    if !visited.insert(role.to_string()) {
        return None;
    }
    let meta_exists = |name: &str| playbooks_dir().join(format!("{name}.meta.yml")).is_file();
    if meta_exists(role) {
        return Some(role.to_string());
    }
    for path in playbook_files() {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if playbook_roles(&path).iter().any(|r| r == role) && meta_exists(stem) {
            return Some(stem.to_string());
        }
    }
    for dependent in all_roles() {
        if role_dependencies(&dependent).iter().any(|dep| dep == role)
            && let Some(app) = resolve_app(&dependent, visited)
        {
            return Some(app);
        }
    }
    None
}

/// The roles a role's `meta/main.yml` pulls in as dependencies.
fn role_dependencies(role: &str) -> Vec<String> {
    let path = role_dir(role).join("meta/main.yml");
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let parsed: Mapping =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let Some(deps) = field(&parsed, "dependencies").and_then(Value::as_sequence) else {
        return Vec::new();
    };
    deps.iter()
        .filter_map(|entry| {
            entry.as_str().map(str::to_string).or_else(|| {
                entry
                    .get("role")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .collect()
}

fn playbook_roles(path: &Path) -> Vec<String> {
    let mut roles = Vec::new();
    let plays = parse_yaml(path)
        .as_sequence()
        .cloned()
        .unwrap_or_else(|| panic!("{} must hold a sequence of plays", path.display()));
    for play in &plays {
        let Some(list) = play.get("roles").and_then(Value::as_sequence) else {
            continue;
        };
        for entry in list {
            let name = entry.as_str().map(str::to_string).or_else(|| {
                entry
                    .get("role")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            if let Some(name) = name {
                roles.push(name);
            }
        }
    }
    roles
}

/// Every `units:` declaration across the committed Playbook Metas.
pub fn declared_units() -> BTreeSet<OwnedUnit> {
    let mut out = BTreeSet::new();
    for (app, path) in meta_files() {
        let parsed = parse_yaml(&path);
        let meta = parsed
            .as_mapping()
            .unwrap_or_else(|| panic!("{} must hold a mapping", path.display()));
        let Some(units) = field(meta, "units").and_then(Value::as_sequence) else {
            continue;
        };
        for decl in units {
            let (name, scope) = match decl {
                Value::String(name) => (name.clone(), "system".to_string()),
                Value::Mapping(scoped) => (
                    field(scoped, "name")
                        .and_then(Value::as_str)
                        .unwrap_or_else(|| panic!("{app}: a scoped unit declares `name`"))
                        .to_string(),
                    field(scoped, "scope")
                        .and_then(Value::as_str)
                        .unwrap_or_else(|| panic!("{app}: a scoped unit declares `scope`"))
                        .to_string(),
                ),
                other => panic!("{app}: {other:?} is not a unit declaration"),
            };
            out.insert(OwnedUnit {
                app: app.to_string(),
                // The crate's own qualifier, not a copy of its rule. Called
                // without the `{admin_user}` substitution `owned_units` does
                // first, so the comparison stays host-independent — the
                // placeholder is what DECLARED_WITHOUT_FILE names syncthing's
                // unit by.
                unit: qualified_unit_name(&name),
                scope: match scope.as_str() {
                    "system" => Scope::System,
                    "user" => Scope::User,
                    other => panic!("{app}: `{other}` is not a unit scope"),
                },
            });
        }
    }
    out
}
