use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn roles_dir() -> PathBuf {
    repo_root().join("ansible/roles")
}

fn playbooks_dir() -> PathBuf {
    repo_root().join("ansible/playbooks")
}

/// The role a play must run in `post_tasks` once any of its roles can restart caddy.
const GATE_ROLE: &str = "ingress_gate";

/// The handler name every vhost writer notifies.
const RESTART_HANDLER: &str = "Restart caddy";

fn field<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a Value> {
    mapping.get(Value::String(key.to_string()))
}

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yaml(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "yml") {
            out.push(path);
        }
    }
}

/// Roles with a task that notifies `Restart caddy`. A restart is what makes a bad
/// vhost fatal: `caddy reload` validates and keeps the running config, but a restart
/// replaces it, so a vhost binding an address the host does not own takes every other
/// vhost down with it.
fn roles_that_restart_caddy() -> BTreeSet<String> {
    let mut roles = BTreeSet::new();
    for entry in fs::read_dir(roles_dir()).expect("ansible/roles must exist") {
        let role_dir = entry.unwrap().path();
        let Some(role) = role_dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let mut task_files = Vec::new();
        collect_yaml(&role_dir.join("tasks"), &mut task_files);
        let notifies = task_files
            .iter()
            .any(|file| fs::read_to_string(file).unwrap().contains(RESTART_HANDLER));
        if notifies {
            roles.insert(role.to_string());
        }
    }
    roles
}

fn playbook_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(playbooks_dir())
        .expect("ansible/playbooks must exist")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            name.ends_with(".yml") && !name.ends_with(".meta.yml")
        })
        .collect();
    files.sort();
    files
}

fn plays(path: &Path) -> Vec<Mapping> {
    let doc: Value = serde_yaml::from_str(&fs::read_to_string(path).unwrap())
        .unwrap_or_else(|err| panic!("{} is not valid YAML: {err}", path.display()));
    doc.as_sequence()
        .unwrap_or_else(|| panic!("{} is not a sequence of plays", path.display()))
        .iter()
        .map(|play| {
            play.as_mapping()
                .unwrap_or_else(|| panic!("{} holds a play that is not a mapping", path.display()))
                .clone()
        })
        .collect()
}

fn play_roles(play: &Mapping) -> Vec<String> {
    let Some(roles) = field(play, "roles").and_then(Value::as_sequence) else {
        return Vec::new();
    };
    roles
        .iter()
        .filter_map(|entry| match entry {
            Value::String(name) => Some(name.clone()),
            Value::Mapping(mapping) => field(mapping, "role")
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .collect()
}

/// `true` when the play's `post_tasks` include the gate role with `always` applied to
/// the role's own tasks.
///
/// `apply:` is load-bearing. Tags written on an `include_role` task cover the include
/// itself, not the tasks it pulls in, so `tags: [always]` alone leaves the gate silently
/// skipped on every tag-limited run — `auberge deploy actual`, the exact deploy shape
/// that took the fleet down in #568.
fn gate_is_wired(play: &Mapping) -> bool {
    let Some(post_tasks) = field(play, "post_tasks").and_then(Value::as_sequence) else {
        return false;
    };
    post_tasks
        .iter()
        .filter_map(Value::as_mapping)
        .filter_map(|task| field(task, "ansible.builtin.include_role"))
        .filter_map(Value::as_mapping)
        .filter(|include| field(include, "name").and_then(Value::as_str) == Some(GATE_ROLE))
        .any(|include| {
            field(include, "apply")
                .and_then(Value::as_mapping)
                .and_then(|apply| field(apply, "tags"))
                .and_then(Value::as_sequence)
                .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some("always")))
        })
}

#[test]
fn test_roles_that_restart_caddy_are_discovered() {
    let roles = roles_that_restart_caddy();
    assert!(
        roles.contains("caddy"),
        "the caddy role notifies `{RESTART_HANDLER}`; discovery found {roles:?}"
    );
    assert!(
        roles.len() > 1,
        "app roles write vhosts and notify `{RESTART_HANDLER}`; discovery found {roles:?}"
    );
}

#[test]
fn test_every_play_that_restarts_caddy_gates_on_ingress() {
    let restarters = roles_that_restart_caddy();
    assert!(
        roles_dir().join(GATE_ROLE).join("tasks/main.yml").exists(),
        "the playbooks include `{GATE_ROLE}`, so the role must exist"
    );

    for path in playbook_files() {
        let playbook = path.file_name().unwrap().to_str().unwrap().to_string();
        for (index, play) in plays(&path).iter().enumerate() {
            let triggers: Vec<String> = play_roles(play)
                .into_iter()
                .filter(|role| restarters.contains(role))
                .collect();
            if triggers.is_empty() {
                continue;
            }
            assert!(
                gate_is_wired(play),
                "{playbook} play {index} runs {triggers:?}, which restart caddy, \
                 but has no `{GATE_ROLE}` post_task applying the `always` tag. \
                 Without it the play can leave every vhost on the host dark and still \
                 report success (#568)."
            );
        }
    }
}
