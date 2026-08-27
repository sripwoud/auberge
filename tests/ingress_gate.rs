use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_yaml::{Mapping, Value};

mod common;

use common::{all_roles, field, playbook_files, role_dir, yml_files};

/// The role a play must run in `post_tasks` once any of its roles can restart caddy.
const GATE_ROLE: &str = "ingress_gate";

/// The handler name every vhost writer notifies.
const RESTART_HANDLER: &str = "Restart caddy";

/// Roles with a task that notifies `Restart caddy`. A restart is what makes a bad
/// vhost fatal: `caddy reload` validates and keeps the running config, but a restart
/// replaces it, so a vhost binding an address the host does not own takes every other
/// vhost down with it.
///
/// Read as text rather than as parsed `notify:` lists on purpose. This is the
/// discovery half of the fence, and it is allowed to over-report: a role that
/// merely mentions the handler is gated too, which costs a `post_task` and
/// nothing else. Under-reporting is what takes the fleet down, so the loose
/// test is the safe direction — and
/// `test_roles_that_restart_caddy_are_discovered` is what stops it from
/// silently reporting nobody.
fn roles_that_restart_caddy() -> BTreeSet<String> {
    all_roles()
        .into_iter()
        .filter(|role| {
            yml_files(&role_dir(role).join("tasks"))
                .iter()
                .any(|file| fs::read_to_string(file).unwrap().contains(RESTART_HANDLER))
        })
        .collect()
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
        role_dir(GATE_ROLE).join("tasks/main.yml").exists(),
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
