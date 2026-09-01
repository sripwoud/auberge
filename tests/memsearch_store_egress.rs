//! What the agent Host's memory store is, and what leaves it.
//!
//! ADR-0054 makes ruche a zero-backup Host: agent memories survive the box
//! because they are markdown a Syncthing folder replicates to lechuck, and the
//! vector index is rebuildable from that markdown alone. Five declarations
//! carry that property and no single file states it, which is why they are
//! fenced together rather than reviewed apart:
//!
//! - `memsearch.meta.yml` declares no Backup Recipe. The immich precedent
//!   (`test_immich_meta_declares_no_backup_recipe`, `src/playbook_meta.rs`)
//!   holds an absence in place the only way an absence can be held — by
//!   asserting it;
//! - the folder the run replicates is the memory directory the `memsearch` role
//!   creates. Read transitively, through the default the playbook's expression
//!   consumes, because a `vars:` block naming a key proves nothing about which
//!   directory exists on the Host;
//! - the Milvus index lies outside that folder. Replicating a file being
//!   written is corruption, not backup, and it is the one way this design turns
//!   a disposable index into a corrupted store;
//! - the run announces nowhere. The tailnet ACL denies `tag:agent ->
//!   tag:trusted` (ADR-0055), so ruche could not dial lechuck even if it tried;
//!   discovery left on would have it trying, over the public internet, through
//!   third-party announce and relay servers a zero-external-trust box has no
//!   business reaching;
//! - every Syncthing write compares against the running configuration first.
//!   The role owns declarations in a file Syncthing itself rewrites, and a
//!   `PUT` answers 200 whether or not it changed anything, so an unguarded
//!   write reports `changed` on every deploy of every Host running the role.

use std::collections::{BTreeMap, BTreeSet};

use serde_yaml::{Mapping, Value};

mod common;

use common::{Plays, defaults, field, playbooks_dir, resolve, role_tasks, strings, tasks_in};

/// The playbook whose declarations this fence reads.
const PLAYBOOK: &str = "memsearch.yml";

/// The role entry that declares what leaves the box.
const REPLICATOR: &str = "syncthing";

/// The play-level `roles:` entries of [`PLAYBOOK`], as `(role, vars)`.
///
/// A bare-string entry carries no `vars:` and is yielded with an empty map, so
/// a roster written either way reads the same here — and a role that stopped
/// binding its parameters shows up as a missing key rather than as an absence
/// from the roster.
fn roster() -> Vec<(String, Mapping)> {
    let path = playbooks_dir().join(PLAYBOOK);
    let plays = common::parse_yaml(&path);
    let plays = plays
        .as_sequence()
        .unwrap_or_else(|| panic!("{PLAYBOOK} must hold a sequence of plays"));
    let mut out = Vec::new();
    for play in plays {
        let Some(entries) = play.get("roles").and_then(Value::as_sequence) else {
            continue;
        };
        for entry in entries {
            if let Some(name) = entry.as_str() {
                out.push((name.to_string(), Mapping::new()));
                continue;
            }
            let Some(map) = entry.as_mapping() else {
                panic!("{PLAYBOOK}: {entry:?} is not a roles entry");
            };
            let name = field(map, "role")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{PLAYBOOK}: a mapping roles entry declares `role`"))
                .to_string();
            let vars = field(map, "vars")
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            out.push((name, vars));
        }
    }
    out
}

/// The `vars:` [`PLAYBOOK`] binds on its [`REPLICATOR`] entry.
fn replicator_vars() -> Mapping {
    roster()
        .into_iter()
        .find(|(role, _)| role == REPLICATOR)
        .unwrap_or_else(|| panic!("{PLAYBOOK} must reach the `{REPLICATOR}` role"))
        .1
}

/// One binding off that entry, resolved through the `memsearch` role's defaults
/// — the only place the run's own paths are declared, and therefore the only
/// substitution that can turn an expression into a path.
fn replicator_var(key: &str, vars: &BTreeMap<String, String>) -> String {
    let bindings = replicator_vars();
    let raw = field(&bindings, key)
        .unwrap_or_else(|| panic!("{PLAYBOOK}: the `{REPLICATOR}` entry must bind `{key}`"));
    match raw {
        Value::String(text) => resolve(text, vars),
        Value::Bool(flag) => flag.to_string(),
        other => panic!("{PLAYBOOK}: `{key}` is {other:?}, which resolves to no value"),
    }
}

/// Every directory the `memsearch` role creates, resolved through its defaults.
///
/// `state: directory` is the whole test: a `file` task without it asserts
/// attributes on something another task made, and a fence that counted those
/// would report directories this role does not own.
fn created_directories(vars: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for task in role_tasks("memsearch") {
        let Some(file) = field(&task.body, "ansible.builtin.file").and_then(Value::as_mapping)
        else {
            continue;
        };
        if field(file, "state").and_then(Value::as_str) != Some("directory") {
            continue;
        }
        let path = field(file, "path")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memsearch: a `file` task declares `path`"));
        out.insert(resolve(path, vars));
    }
    out
}

/// Whether a task names `key` anywhere it could act on it — in a field, in a
/// template expression, or in a `when:` it inherited from an enclosing block.
/// Serialized rather than walked, because the answer wanted is "does this task
/// read the name at all", and every shape the name can arrive in is text by
/// the time it reaches Ansible.
fn mentions(task: &common::Task, key: &str) -> bool {
    let body = serde_yaml::to_string(&task.body).expect("a walked task re-serializes");
    body.contains(key) || task.guards.iter().any(|guard| guard.contains(key))
}

/// One entry of a role's `defaults/main.yml`, as text.
///
/// `common::defaults` drops booleans — it exists to resolve `{{ … }}` into
/// paths, and a bool is never one — so a fence asking what a flag defaults to
/// has to read the file. Reading it here rather than widening the shared
/// walker keeps every other fence's resolution exactly as wide as it was.
fn role_default(role: &str, key: &str) -> String {
    let path = common::role_dir(role).join("defaults/main.yml");
    let parsed = common::parse_yaml(&path);
    let map = parsed
        .as_mapping()
        .unwrap_or_else(|| panic!("{} must hold a mapping", common::relative(&path)));
    match field(map, key) {
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::String(text)) => text.clone(),
        // An explicit null is a declaration in its own right — "this role
        // asserts nothing here" — and is spelled back as the `~` the file
        // carries, so a caller can tell it from an absent key, which is not.
        Some(Value::Null) => "~".to_string(),
        Some(other) => panic!("{role}: `{key}` is {other:?}"),
        None => panic!("the `{role}` role must default `{key}`"),
    }
}

#[test]
fn test_the_memsearch_meta_declares_no_backup_recipe() {
    let path = playbooks_dir().join("memsearch.meta.yml");
    let meta = common::parse_yaml(&path);
    let meta = meta
        .as_mapping()
        .unwrap_or_else(|| panic!("{} must hold a mapping", path.display()));
    assert!(
        field(meta, "backup").is_none(),
        "memsearch.meta.yml declares a Backup Recipe. ruche holds no state a \
         backup could be the answer for (ADR-0054): the memories leave the box \
         through the Syncthing folder this run declares, and the index is \
         rebuilt from them. A Recipe here would back up an agent box that is \
         supposed to be reinstallable, and would put the Milvus index — a file \
         being written — into a restic snapshot."
    );
}

#[test]
fn test_the_replicated_folder_is_the_memory_directory_the_role_creates() {
    let vars = defaults("memsearch");
    let replicated = replicator_var("syncthing_workspace_path", &vars);
    let created = created_directories(&vars);
    assert!(
        created.contains(&replicated),
        "{PLAYBOOK} replicates `{replicated}`, which the memsearch role creates \
         nowhere. It creates {created:?}. Syncthing would carry an empty folder \
         off the box while the agents wrote their memories somewhere else, and \
         both halves would look healthy."
    );
}

#[test]
fn test_the_index_is_not_inside_the_replicated_folder() {
    let vars = defaults("memsearch");
    let replicated = replicator_var("syncthing_workspace_path", &vars);
    let index = vars
        .get("memsearch_index_uri")
        .unwrap_or_else(|| panic!("memsearch defaults must declare `memsearch_index_uri`"));
    let index = resolve(index, &vars);
    assert!(
        !index.starts_with(&format!("{replicated}/")) && index != replicated,
        "the Milvus index `{index}` sits inside the replicated folder \
         `{replicated}`. Syncthing would replicate a file memsearch holds open \
         and writes to, which is corruption rather than backup — and it would \
         push the one piece of state ADR-0054 calls disposable off a box whose \
         whole premise is that nothing on it needs saving."
    );
}

#[test]
fn test_the_agent_box_announces_nowhere() {
    let vars = defaults("memsearch");
    assert_eq!(
        replicator_var("syncthing_discovery_enabled", &vars),
        "false",
        "{PLAYBOOK} leaves Syncthing discovery on. The tailnet ACL denies \
         `tag:agent -> tag:trusted` (ADR-0055), so ruche cannot dial lechuck; \
         with discovery on it would keep trying — announcing itself to \
         third-party discovery servers and dialling relays over the public \
         internet, from the one Host assumed compromisable. lechuck initiates, \
         and the config has to say so."
    );

    let consumers: Vec<String> = role_tasks(REPLICATOR)
        .into_iter()
        .filter(|task| mentions(task, "syncthing_discovery_enabled"))
        .map(|task| common::task_name(&task.body).to_string())
        .collect();
    assert!(
        !consumers.is_empty(),
        "no task in the `{REPLICATOR}` role reads `syncthing_discovery_enabled`, \
         so {PLAYBOOK} binding it to `false` changes nothing on the Host. A \
         switch nothing is wired to reads exactly like one that works."
    );

    let role_default = role_default(REPLICATOR, "syncthing_discovery_enabled");
    assert_eq!(
        role_default, "~",
        "the `{REPLICATOR}` role defaults `syncthing_discovery_enabled` to \
         `{role_default}` instead of leaving it unset. Syncthing's peer \
         discovery is also configured by hand in its web UI, so a role that \
         enforces a default here undoes a Host hardened to announce nowhere on \
         its next `apps.yml` deploy. Unset means the role asserts nothing; \
         ruche is the only Host that declares anything."
    );
}

/// The API block does not run under `--check`.
///
/// `ansible.builtin.uri` declares no check mode, so under `--check` every read
/// is skipped and `syncthing_running` is a skip result with no `.json`. Every
/// guard below it dereferences that, so the play would die rather than report
/// — on `apps.yml`, which runs this role against the data fleet, as much as on
/// the agent Host's own run. `--check` is how a change is inspected before it
/// lands, so a role that cannot survive it cannot be inspected.
#[test]
fn test_the_syncthing_api_block_is_skipped_under_check_mode() {
    let mut seen = 0;
    for task in role_tasks(REPLICATOR) {
        if field(&task.body, "ansible.builtin.uri").is_none() {
            continue;
        }
        seen += 1;
        let name = common::task_name(&task.body);
        assert!(
            task.guards
                .iter()
                .any(|guard| guard.contains("ansible_check_mode")),
            "`{name}` reaches the API with no `ansible_check_mode` guard above \
             it: its guards are {:?}. Under `--check` the `uri` reads are \
             skipped and the derived comparisons dereference a skip result.",
            task.guards
        );
    }
    assert!(
        seen >= 6,
        "the scan found {seen} `uri` tasks in the `{REPLICATOR}` role; it pings, \
         reads the config, reads its own device ID and writes four \
         declarations, so a smaller count means the walk stopped seeing them."
    );
}

/// Every name the run's own bindings still carry after the `memsearch` role's
/// defaults have been substituted, checked to be one the user can answer.
///
/// An expression nothing resolves compares equal to nothing, so it would pass
/// every assertion above it while naming a folder that does not exist. Two
/// things can answer one: a default of a role in this run, which `resolve` has
/// already substituted, or a Key Registry name, which is the only answer the
/// operator supplies and the only vocabulary `auberge config init` offers.
#[test]
fn test_every_binding_this_fence_reads_is_answerable() {
    let vars = defaults("memsearch");
    let registry = common::registry_keys();
    let declared = playbooks_dir().join("memsearch.meta.yml");
    let meta = common::parse_yaml(&declared);
    let required: BTreeSet<String> = strings(
        meta.as_mapping()
            .and_then(|map| field(map, "required_keys")),
    )
    .into_iter()
    .collect();

    for key in [
        "syncthing_workspace_path",
        "syncthing_workspace_id",
        "syncthing_workspace_label",
        "syncthing_device_id",
        "syncthing_device_name",
        "syncthing_configure_workspace",
        "syncthing_discovery_enabled",
    ] {
        let value = replicator_var(key, &vars);
        assert!(!value.is_empty(), "{PLAYBOOK}: `{key}` binds nothing");
        for name in unresolved_names(&value) {
            assert!(
                registry.contains(&name),
                "{PLAYBOOK}: `{key}` is `{value}`, and `{name}` is neither a \
                 default of this run nor a Key Registry name — nothing can \
                 answer it, and the play dies on an undefined variable the \
                 first time a task reads it."
            );
            assert!(
                required.contains(&name),
                "{PLAYBOOK} reads the registry key `{name}` and \
                 memsearch.meta.yml does not declare it in `required_keys`. \
                 Preflight would pass a Config that never had to hold it \
                 (ADR-0045)."
            );
        }
    }

    assert_eq!(
        replicator_var("syncthing_configure_workspace", &vars),
        "true",
        "{PLAYBOOK} declares a workspace folder's parameters and leaves \
         `syncthing_configure_workspace` off, so the role skips every task that \
         would write them into Syncthing's configuration."
    );
}

/// The `{{ name }}` references a string still carries, bare names only: a
/// filtered or compound expression is not a name this fence can vouch for, and
/// reporting one as unanswerable would be a false alarm.
fn unresolved_names(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find("{{") {
        let Some(offset) = rest[open..].find("}}") else {
            return out;
        };
        let close = open + offset;
        let inner = rest[open + 2..close].trim();
        if inner
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.push(inner.to_string());
        }
        rest = &rest[close + 2..];
    }
    out
}

/// Guards against the walk this fence depends on going quiet: `tasks_in` over a
/// playbook needs [`Plays::Descend`], and `strings` is how a multi-valued field
/// is read. Both are imported above; a compile that stopped using them would
/// leave the assertions reading nothing.
#[test]
fn test_the_playbook_walk_reaches_the_run() {
    let path = playbooks_dir().join(PLAYBOOK);
    let names: Vec<String> = tasks_in(&path, Plays::Descend)
        .iter()
        .map(|task| common::task_name(&task.body).to_string())
        .collect();
    let roles = roster();
    assert!(
        roles.iter().any(|(role, _)| role == "memsearch"),
        "{PLAYBOOK} must reach the `memsearch` role; it reaches {roles:?} and \
         its tasks are {names:?}"
    );
    assert!(
        strings(field(&replicator_vars(), "syncthing_workspace_path")).len() == 1,
        "{PLAYBOOK}: the replicated folder must be one path"
    );
}

/// Every write the `syncthing` role makes over the API is guarded by a
/// convergence check.
///
/// The role reads the running configuration once and writes only what differs,
/// which is the whole reason it can own Syncthing's own file at all: a `PUT`
/// or `PATCH` answers 200 whether or not it changed anything, so an unguarded
/// write reports `changed` on every deploy forever — and a deploy that is
/// always changed is one nobody reads. The guard has to name a `_is_current`
/// variable, because that is where the comparison against the running
/// configuration lives; a guard on anything else is not one.
#[test]
fn test_every_syncthing_api_write_is_guarded_by_a_convergence_check() {
    let mut writes = 0;
    for task in role_tasks(REPLICATOR) {
        let Some(uri) = field(&task.body, "ansible.builtin.uri").and_then(Value::as_mapping) else {
            continue;
        };
        let method = field(uri, "method")
            .and_then(Value::as_str)
            .unwrap_or("GET");
        if !matches!(method, "PUT" | "PATCH" | "POST") {
            continue;
        }
        writes += 1;
        let name = common::task_name(&task.body);
        assert!(
            task.guards
                .iter()
                .any(|guard| guard.contains("_is_current")),
            "`{name}` sends {method} with no convergence check: its guards are \
             {:?}. Syncthing answers 200 whether the write changed anything, so \
             this task reports `changed` on every deploy of every Host running \
             the role.",
            task.guards
        );
    }
    assert!(
        writes >= 4,
        "the scan found {writes} API writes in the `{REPLICATOR}` role; it \
         declares a web-UI address, peer discovery, a remote device and a \
         replicated folder, so a smaller count means the walk stopped seeing \
         them and this fence is asserting over nothing."
    );
}
