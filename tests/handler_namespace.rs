use std::collections::{BTreeMap, BTreeSet};
use std::fs;

mod common;

use common::{playbook_files, role_dir, yml_files};

/// Every key of a handler except `name`, sorted and YAML-serialized, so two
/// definitions compare equal exactly when they declare the same thing —
/// whatever order the keys were written in.
fn definition(handler: &serde_yaml::Mapping) -> String {
    handler
        .iter()
        .filter(|(key, _)| *key != &serde_yaml::Value::from("name"))
        .map(|(key, value)| {
            let render =
                |v: &serde_yaml::Value| serde_yaml::to_string(v).unwrap().trim().to_string();
            (render(key), render(value))
        })
        .collect::<BTreeMap<_, _>>()
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A role's handlers, as name -> definition.
fn role_handlers(role: &str) -> BTreeMap<String, String> {
    let path = role_dir(role).join("handlers/main.yml");
    if !path.exists() {
        return BTreeMap::new();
    }
    let handlers: Option<Vec<serde_yaml::Mapping>> =
        serde_yaml::from_str(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    handlers
        .unwrap_or_default()
        .iter()
        .map(|handler| {
            let name = handler
                .get(serde_yaml::Value::from("name"))
                .and_then(|name| name.as_str())
                .unwrap_or_else(|| panic!("{}: handler without a name", path.display()))
                .to_string();
            (name, definition(handler))
        })
        .collect()
}

/// Roles a role pulls in with `include_role` — their handlers join the play too.
fn included_roles(role: &str) -> BTreeSet<String> {
    let mut included = BTreeSet::new();

    for file in yml_files(&role_dir(role).join("tasks")) {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("include_role:") {
                continue;
            }
            let name = lines[i + 1..]
                .iter()
                .find_map(|l| l.trim().strip_prefix("name:"))
                .unwrap_or_else(|| panic!("{role}: include_role without a name"));
            included.insert(name.trim().trim_matches('"').to_string());
        }
    }

    included
}

/// Every play in the repo, as (label, the roles it loads). `include_role` is
/// followed transitively: an included role's handlers land in the same play.
fn plays() -> Vec<(String, BTreeSet<String>)> {
    let mut plays = Vec::new();

    for path in playbook_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let parsed: Vec<serde_yaml::Value> =
            serde_yaml::from_str(&fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        for play in parsed {
            let listed = play.get("roles").and_then(|r| r.as_sequence());
            let mut pending: Vec<String> = listed
                .into_iter()
                .flatten()
                .map(|role| {
                    role.as_str()
                        .or_else(|| role.get("role").and_then(|r| r.as_str()))
                        .unwrap_or_else(|| panic!("{}: unreadable role entry", path.display()))
                        .to_string()
                })
                .collect();

            let mut roles = BTreeSet::new();
            while let Some(role) = pending.pop() {
                if roles.insert(role.clone()) {
                    pending.extend(included_roles(&role));
                }
            }

            let label = play
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unnamed play");
            plays.push((format!("{name} ({label})"), roles));
        }
    }

    plays
}

/// Ansible handler names live in one global namespace per play: when two roles
/// loaded by the same play define the same name, the last one loaded wins and
/// shadows the rest. Sharing a name is only safe when both definitions declare
/// the same thing — a shadow with a divergent `when:` guard skips silently, so
/// the notify becomes a no-op rather than an error (issue #569: baikal's pool
/// restart resolved to yourls's handler, whose guard is false on a
/// baikal-scoped run, and /run/php/baikal-fpm.sock never appeared).
#[test]
fn test_handlers_shared_within_a_play_declare_the_same_thing() {
    let mut violations = Vec::new();

    for (play, roles) in plays() {
        let mut by_name: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for role in &roles {
            for (name, definition) in role_handlers(role) {
                by_name
                    .entry(name)
                    .or_default()
                    .insert(role.clone(), definition);
            }
        }

        for (name, by_role) in by_name {
            if by_role.values().collect::<BTreeSet<_>>().len() < 2 {
                continue;
            }
            let definitions: String = by_role
                .iter()
                .map(|(role, definition)| {
                    format!("\n    {role}: {}", definition.replace('\n', "; "))
                })
                .collect();
            violations.push(format!(
                "{play}: handler `{name}` declares something different per role, so the \
                 last role loaded shadows the others — give each one a role-scoped \
                 name:{definitions}"
            ));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// What the fence above cannot say for itself.
///
/// It reports a violation only where two roles loaded by one play define the
/// same handler name and disagree about it. `violations.is_empty()` is exactly
/// as true of a scan that reached no play, no role, or no handler — so the
/// assertion carries no weight until something states that the walk arrives
/// somewhere. This is that statement, and it is why `included_roles` may read
/// the tree through the shared walk at all: widening a walk under a fence with
/// no floor widens nothing anybody would notice.
#[test]
fn test_the_scan_reaches_plays_handlers_and_a_name_two_roles_share() {
    let plays = plays();
    assert!(
        !plays.is_empty(),
        "no play came back from ansible/playbooks"
    );

    let definitions: usize = plays
        .iter()
        .flat_map(|(_, roles)| roles)
        .map(|role| role_handlers(role).len())
        .sum();
    assert!(
        definitions > 0,
        "the plays load roles, and not one of them yielded a handler"
    );

    let shared = plays
        .iter()
        .filter(|(_, roles)| {
            let mut by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for role in roles {
                for name in role_handlers(role).into_keys() {
                    by_name.entry(name).or_default().insert(role.clone());
                }
            }
            by_name.values().any(|roles| roles.len() > 1)
        })
        .count();
    assert!(
        shared > 0,
        "no play loads two roles that define the same handler name, so the \
         comparison this file exists to make never runs"
    );

    assert!(
        included_roles("baikal").contains("dns_record"),
        "baikal pulls in `dns_record` with `include_role`, whose handlers join \
         the same play; the transitive half of the scan stopped seeing it"
    );
}
