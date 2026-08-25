//! Fleet-wide fence on Unit Ownership: which systemd units each App answers
//! for, declared as `units:` in its Playbook Meta.
//!
//! A failed deploy reads that declaration to report the state it left the
//! App's units in (#644) — a fact the CLI could not previously reach: a
//! Backup Recipe's `systemd_services` is a quiesce order 11 Apps have, and
//! `memory:` keys are opt-in per budget. Neither is an inventory.
//!
//! The declaration is hand-written, so it is fenced the way ADR-0028, 0035,
//! 0038 and 0040 fence theirs: everything the repo's own tasks reveal —
//! every unit file a role templates or copies, and every unit a role drops
//! in over, since a drop-in names the unit it refines — is computed here and
//! must be declared, in both directions. A unit a role installs without
//! either (a packaged template unit it only enables) cannot be computed off
//! any file, so it is declared with the reason the scan cannot see it.
//!
//! Deliberately outside the domain: units an App merely starts or depends on
//! (postgresql, redis, mariadb, docker, tailscaled) — they are shared
//! substrate with their own owners, not the App — and php-fpm, whose unit
//! name (`php8.4-fpm`) is a play-time package fact the roles themselves
//! discover from `package_facts`, so a Meta declaration of it would drift on
//! every PHP transition.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Sequence, Value};

/// A unit an App declares that no role installs a file for, and why the scan
/// cannot compute it. Each entry is checked to stay underivable: the day a
/// role starts templating it, the entry must go.
const DECLARED_WITHOUT_FILE: &[(&str, &str, &str)] = &[(
    "syncthing",
    "syncthing@{admin_user}.service",
    "a packaged template unit the role only enables per user; there is no \
     file to install, so no task reveals it",
)];

/// systemd's own closed set of unit types, mirrored from
/// `src/playbook_meta.rs`.
const UNIT_TYPE_SUFFIXES: &[&str] = &[
    ".automount",
    ".device",
    ".mount",
    ".path",
    ".scope",
    ".service",
    ".slice",
    ".socket",
    ".swap",
    ".target",
    ".timer",
];

fn ansible_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible")
}

fn roles_dir() -> PathBuf {
    ansible_dir().join("roles")
}

fn playbooks_dir() -> PathBuf {
    ansible_dir().join("playbooks")
}

fn role_dir(role: &str) -> PathBuf {
    roles_dir().join(role)
}

fn all_roles() -> Vec<String> {
    let mut roles: Vec<String> = fs::read_dir(roles_dir())
        .expect("ansible/roles must exist")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    roles.sort();
    roles
}

fn field<'a>(task: &'a Mapping, key: &str) -> Option<&'a Value> {
    task.get(Value::from(key))
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Sequence(many)) => many
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn flatten(tasks: &Sequence, out: &mut Vec<Mapping>) {
    for task in tasks {
        let Some(body) = task.as_mapping() else {
            continue;
        };
        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = field(body, section).and_then(Value::as_sequence) {
                flatten(inner, out);
                nested = true;
            }
        }
        if !nested {
            out.push(body.clone());
        }
    }
}

/// Every task in the role, across all of its task files. A unit installed
/// under a guard is still a unit a failed deploy has to answer for.
fn every_task(role: &str) -> Vec<Mapping> {
    let mut files: Vec<PathBuf> = fs::read_dir(role_dir(role).join("tasks"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut tasks = Vec::new();
    for file in files {
        let raw = fs::read_to_string(&file).expect("a listed task file must be readable");
        let parsed: Sequence = serde_yaml::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} must parse: {e}", file.display()));
        flatten(&parsed, &mut tasks);
    }
    tasks
}

fn defaults(role: &str) -> BTreeMap<String, String> {
    let path = role_dir(role).join("defaults/main.yml");
    let Ok(raw) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let parsed: Mapping =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    parsed
        .iter()
        .filter_map(|(key, value)| {
            let key = key.as_str()?.to_string();
            match value {
                Value::String(text) => Some((key, text.clone())),
                Value::Number(number) => Some((key, number.to_string())),
                _ => None,
            }
        })
        .collect()
}

fn substitute(input: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = input;
    loop {
        let Some(open) = rest.find("{{") else {
            out.push_str(rest);
            return out;
        };
        let Some(offset) = rest[open..].find("}}") else {
            out.push_str(rest);
            return out;
        };
        let close = open + offset;
        match vars.get(rest[open + 2..close].trim()) {
            Some(value) => {
                out.push_str(&rest[..open]);
                out.push_str(value);
            }
            None => out.push_str(&rest[..close + 2]),
        }
        rest = &rest[close + 2..];
    }
}

fn resolve(raw: &str, vars: &BTreeMap<String, String>) -> String {
    let mut current = raw.to_string();
    for _ in 0..10 {
        let next = substitute(&current, vars);
        if next == current {
            return current;
        }
        current = next;
    }
    panic!("{raw} does not resolve to a fixed point");
}

/// One unit an App owns: its `systemctl` name and the manager it lives in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Owned {
    app: String,
    unit: String,
    user_scope: bool,
}

impl Owned {
    fn id(&self) -> String {
        let scope = if self.user_scope { " (user)" } else { "" };
        format!("{}/{}{}", self.app, self.unit, scope)
    }
}

/// The unit a `dest` reveals, if it lands in a systemd unit directory this
/// model reads — the system directory or a user's — either as the unit file
/// itself or as a drop-in under `<unit>.<type>.d/`, which names the unit it
/// refines just as well. Returns the unit and whether it is user-scoped.
fn unit_configured_at(dest: &str) -> Option<(String, bool)> {
    let (dir, file) = dest.rsplit_once('/')?;
    let scope_of = |path: &str| -> Option<bool> {
        if path == "/etc/systemd/system" {
            Some(false)
        } else if path.ends_with("/.config/systemd/user") {
            Some(true)
        } else {
            None
        }
    };

    let (unit, user_scope) = if let Some(user_scope) = scope_of(dir) {
        (file.to_string(), user_scope)
    } else {
        let (parent, unit_dir) = dir.rsplit_once('/')?;
        let unit = unit_dir.strip_suffix(".d")?;
        let user_scope = scope_of(parent)?;
        if !file.ends_with(".conf") {
            return None;
        }
        (unit.to_string(), user_scope)
    };

    // Asserted before the suffix test: an unresolved name would fail that
    // test and drop out of the domain silently, the one way a new unit could
    // enter the fleet without entering this fence.
    assert!(
        !unit.contains("{{"),
        "`{dest}` configures a systemd unit whose name does not resolve; teach \
         this test how to expand it before relying on it"
    );
    if !UNIT_TYPE_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
    {
        return None;
    }
    Some((unit, user_scope))
}

/// Every unit a role's own tasks reveal, through the file it installs or the
/// drop-in it lays over a packaged one. Unlike the fences that read a unit
/// file's directives, this one needs only the name, so a `dest` reveals its
/// unit whether the body comes from `src` or inline `content:`.
fn units_installed_by(role: &str, vars: &BTreeMap<String, String>) -> BTreeSet<(String, bool)> {
    let mut out = BTreeSet::new();
    for task in every_task(role) {
        for module in ["ansible.builtin.template", "ansible.builtin.copy"] {
            let Some(args) = field(&task, module).and_then(Value::as_mapping) else {
                continue;
            };
            let Some(dest) = field(args, "dest").and_then(Value::as_str) else {
                continue;
            };
            let items = strings(field(&task, "loop"));
            let expansions: Vec<String> = if items.is_empty() {
                vec![dest.to_string()]
            } else {
                items
                    .iter()
                    .map(|item| dest.replace("{{ item }}", item))
                    .collect()
            };
            for dest in expansions {
                if let Some(found) = unit_configured_at(&resolve(&dest, vars)) {
                    out.insert(found);
                }
            }
        }
    }
    out
}

/// The App each role answers as: the role's own name where a Meta of that
/// name exists, otherwise the playbook that runs the role, otherwise the App
/// of a role that depends on it — claude_code_remote deploys as a dependency
/// of the vibecoder role, through vibecoder.yml, so its Meta is vibecoder's.
/// A role that installs units and maps to no Meta is a hard stop: its units
/// would have nowhere to be declared.
fn app_of(role: &str, visited: &mut BTreeSet<String>) -> Option<String> {
    if !visited.insert(role.to_string()) {
        return None;
    }
    let meta_exists = |name: &str| playbooks_dir().join(format!("{name}.meta.yml")).is_file();
    if meta_exists(role) {
        return Some(role.to_string());
    }
    for entry in fs::read_dir(playbooks_dir()).expect("playbooks dir must exist") {
        let path = entry
            .expect("a playbooks dir entry must be readable")
            .path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.ends_with(".meta") || path.extension().is_none_or(|ext| ext != "yml") {
            continue;
        }
        if playbook_roles(&path).iter().any(|r| r == role) && meta_exists(stem) {
            return Some(stem.to_string());
        }
    }
    for dependent in all_roles() {
        if role_dependencies(&dependent).iter().any(|dep| dep == role)
            && let Some(app) = app_of(&dependent, visited)
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

fn playbook_roles(path: &PathBuf) -> Vec<String> {
    let raw = fs::read_to_string(path).expect("a playbook must be readable");
    let docs: Vec<Value> =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let mut roles = Vec::new();
    for doc in &docs {
        let Some(list) = doc.get("roles").and_then(Value::as_sequence) else {
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

/// Every unit the fleet's own tasks reveal, keyed by the App that must
/// declare it.
fn computed_units() -> BTreeSet<Owned> {
    let mut out = BTreeSet::new();
    for role in all_roles() {
        let installed = units_installed_by(&role, &defaults(&role));
        if installed.is_empty() {
            continue;
        }
        let app = app_of(&role, &mut BTreeSet::new()).unwrap_or_else(|| {
            panic!(
                "{role} installs systemd units but maps to no Playbook Meta; \
                 create `<app>.meta.yml` so the units have somewhere to be \
                 declared"
            )
        });
        for (unit, user_scope) in installed {
            out.insert(Owned {
                app: app.clone(),
                unit,
                user_scope,
            });
        }
    }
    out
}

/// The declared unit as `systemctl` addresses it: an explicit unit type is
/// kept, a bare name is a `.service` — mirrored from
/// `playbook_meta::qualified_unit_name`, with `{admin_user}` left standing
/// so the comparison stays host-independent.
fn qualified(name: &str) -> String {
    if UNIT_TYPE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        name.to_string()
    } else {
        format!("{name}.service")
    }
}

/// Every `units:` declaration across the committed Playbook Metas.
fn declared_units() -> BTreeSet<Owned> {
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(playbooks_dir()).expect("playbooks dir must exist") {
        let path = entry
            .expect("a playbooks dir entry must be readable")
            .path();
        let Some(app) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".meta.yml"))
        else {
            continue;
        };
        let raw = fs::read_to_string(&path).expect("a Meta must be readable");
        let meta: Mapping = serde_yaml::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
        let Some(units) = field(&meta, "units").and_then(Value::as_sequence) else {
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
            out.insert(Owned {
                app: app.to_string(),
                unit: qualified(&name),
                user_scope: match scope.as_str() {
                    "system" => false,
                    "user" => true,
                    other => panic!("{app}: `{other}` is not a unit scope"),
                },
            });
        }
    }
    out
}

fn ids(units: &BTreeSet<Owned>) -> BTreeSet<String> {
    units.iter().map(Owned::id).collect()
}

/// Computed -> declared. A role that installs or drops in over a unit has
/// revealed it; the App must own up to it, or a failed deploy of exactly
/// that App reads out nothing — the silence #644 exists to end.
#[test]
fn test_every_unit_a_roles_tasks_reveal_is_declared_by_its_app() {
    let computed = ids(&computed_units());
    let declared = ids(&declared_units());
    assert_eq!(
        computed.difference(&declared).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "roles reveal units their Apps do not declare; add them to the App's \
         `units:` in its Playbook Meta"
    );
}

/// Declared -> computed, with the underivable remainder named. A declaration
/// no task backs is either a packaged unit with its reason listed in
/// DECLARED_WITHOUT_FILE, or a claim about the fleet nobody checks.
#[test]
fn test_every_declared_unit_is_revealed_by_a_task_or_names_why_not() {
    let computed = ids(&computed_units());
    let declared = ids(&declared_units());
    let excused: BTreeSet<String> = DECLARED_WITHOUT_FILE
        .iter()
        .map(|(app, unit, _)| format!("{app}/{unit}"))
        .collect();

    let surplus: BTreeSet<String> = declared.difference(&computed).cloned().collect();
    assert_eq!(
        surplus.difference(&excused).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "Metas declare units no role's tasks reveal and no DECLARED_WITHOUT_FILE \
         entry excuses; either the unit is gone or the scan stopped seeing it — \
         the second is the dangerous one"
    );
    assert_eq!(
        excused.difference(&surplus).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "DECLARED_WITHOUT_FILE excuses units that are either no longer declared \
         or now revealed by a task; drop the stale entry"
    );
}

/// The one ownership fact the scan cannot check per unit: an App that
/// installs no units at all declares none, so a `units:` key on it would be
/// an inventory of nothing. Pinned so the boundary stays deliberate: yourls
/// runs on php-fpm (a play-time package fact) and mariadb (shared
/// substrate), and wireguard has no units — none of which is this fence's
/// domain.
#[test]
fn test_apps_outside_the_domain_declare_no_units() {
    let declared = declared_units();
    for app in ["yourls", "wireguard", "apps", "infrastructure", "hardening"] {
        assert!(
            !declared.iter().any(|owned| owned.app == app),
            "{app} declares units; its exclusion from the ownership domain was \
             deliberate — revisit the doc comment at the top of this file \
             before changing it"
        );
    }
}
