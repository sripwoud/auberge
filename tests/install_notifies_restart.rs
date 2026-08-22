use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Sequence, Value};

/// A version bump replaces an artifact on a Host where the old one is already
/// running, and nothing downstream in the play notices. `state: started` no-ops
/// on a unit systemd reports active (#594), the readiness probe then reads the
/// process it was supposed to validate the replacement of (#598), and the
/// version marker is written whether or not anything restarted (#591). The one
/// task that knows the artifact changed is the task that changed it, so that is
/// where the restart has to be notified from (#599).
///
/// Scope is the version-bump path, recognised by the guard naming the App
/// Version -- `<role>_version`, the convention `version_annotations.rs` already
/// enforces (ADR-0017). The missing-artifact path is deliberately out: an absent
/// `ExecStart` target means the unit is dead, so the `state: started` further
/// down revives it. That is why grimmory's jar download, guarded on a bare
/// `stat` of a versioned dest, is not examined here even though it notifies.
///
/// Modules that carry bytes from elsewhere onto the Host. A `copy` rendering
/// inline `content:` is excluded: that is a note the role authored, not an
/// artifact, and hanging the restart off one is the shape ADR-0027 rejects.
const INSTALL_MODULES: &[&str] = &[
    "ansible.builtin.unarchive",
    "ansible.builtin.get_url",
    "ansible.builtin.copy",
    "ansible.builtin.git",
];

/// Handler modules that can restart a unit.
const SERVICE_MODULES: &[&str] = &[
    "ansible.builtin.systemd_service",
    "ansible.builtin.systemd",
    "ansible.builtin.service",
];

fn roles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles")
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

/// One task with the guards it actually runs under: a `when` on an enclosing
/// block ANDs into every task inside it, which is how `bichon` and `paperless`
/// gate their installs.
struct Task {
    body: Mapping,
    guards: Vec<String>,
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

fn flatten(tasks: &Sequence, inherited: &[String], out: &mut Vec<Task>) {
    for task in tasks {
        let Some(body) = task.as_mapping() else {
            continue;
        };
        let mut scoped = inherited.to_vec();
        scoped.extend(strings(field(body, "when")));
        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = field(body, section).and_then(Value::as_sequence) {
                flatten(inner, &scoped, out);
                nested = true;
            }
        }
        if !nested {
            out.push(Task {
                body: body.clone(),
                guards: scoped,
            });
        }
    }
}

/// Every task in the role, across all of its task files. Order is meaningless
/// here -- this asks which tasks install and which units exist, not what runs
/// when -- so an `include_tasks` needs no resolving to be seen.
fn every_task(role: &str) -> Vec<Task> {
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
        flatten(&parsed, &[], &mut tasks);
    }
    tasks
}

/// The role's scalar defaults, which is where every path a unit runs and every
/// path an install writes is stated (ADR-0027).
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

/// `{{ var }}` replaced by the default it names, until the string stops
/// changing. Anything else -- a filter, a register's field, a name defined
/// somewhere other than defaults -- is left standing, and every caller
/// comparing paths rejects what still holds a `{{`.
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

/// A unit's name as systemd knows it: bare names are services, and a name that
/// already carries a type keeps it.
fn unit_name(name: &str) -> String {
    if name.contains('.') {
        name.to_string()
    } else {
        format!("{name}.service")
    }
}

struct Unit {
    name: String,
    /// The absolute paths its `ExecStart` and `WorkingDirectory` name -- what it
    /// is running, and therefore what replacing means for it.
    runs: Vec<String>,
    /// A `oneshot` without `RemainAfterExit`: it execs its artifact afresh at
    /// every activation, so a replacement is picked up by the next timer firing
    /// with nothing to restart. `immich` is the counter-case -- oneshot, but
    /// `RemainAfterExit` keeps the containers it started alive.
    transient: bool,
}

fn directive_paths(body: &str, vars: &BTreeMap<String, String>) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            ["ExecStart=", "WorkingDirectory="]
                .iter()
                .find_map(|directive| line.strip_prefix(directive))
        })
        .flat_map(|value| {
            resolve(value.trim(), vars)
                .split_whitespace()
                .filter(|token| token.starts_with('/'))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Units the role deploys, read out of the templates it renders into
/// `/etc/systemd/system`.
fn units(role: &str, vars: &BTreeMap<String, String>) -> Vec<Unit> {
    let mut units = Vec::new();
    for task in every_task(role) {
        for module in ["ansible.builtin.template", "ansible.builtin.copy"] {
            let Some(args) = field(&task.body, module).and_then(Value::as_mapping) else {
                continue;
            };
            let (Some(dest), Some(src)) = (
                field(args, "dest").and_then(Value::as_str),
                field(args, "src").and_then(Value::as_str),
            ) else {
                continue;
            };
            if !dest.starts_with("/etc/systemd/system") {
                continue;
            }
            let items = strings(field(&task.body, "loop"));
            let expansions: Vec<(String, String)> = if items.is_empty() {
                vec![(dest.to_string(), src.to_string())]
            } else {
                items
                    .iter()
                    .map(|item| {
                        (
                            dest.replace("{{ item }}", item),
                            src.replace("{{ item }}", item),
                        )
                    })
                    .collect()
            };
            for (dest, src) in expansions {
                let file = src.rsplit('/').next().expect("a src names a file");
                let template = ["templates", "files"]
                    .iter()
                    .map(|dir| role_dir(role).join(dir).join(file))
                    .find(|path| path.is_file())
                    .unwrap_or_else(|| panic!("{role}: {file} is deployed but does not exist"));
                let body = fs::read_to_string(template).expect("a found template must be readable");
                units.push(Unit {
                    name: unit_name(dest.rsplit('/').next().expect("a dest names a file")),
                    runs: directive_paths(&body, vars),
                    transient: body.contains("Type=oneshot") && !body.contains("RemainAfterExit"),
                });
            }
        }
    }
    units
}

/// The units each of the role's handlers restarts. Only `state: restarted`
/// counts: a handler that starts is the no-op this family of bugs is made of.
fn restart_handlers(role: &str, vars: &BTreeMap<String, String>) -> BTreeMap<String, Vec<String>> {
    let path = role_dir(role).join("handlers/main.yml");
    let Ok(raw) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let parsed: Sequence =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));

    let mut handlers = BTreeMap::new();
    for handler in parsed.iter().filter_map(Value::as_mapping) {
        let Some(name) = field(handler, "name").and_then(Value::as_str) else {
            continue;
        };
        for module in SERVICE_MODULES {
            let Some(args) = field(handler, module).and_then(Value::as_mapping) else {
                continue;
            };
            if field(args, "state").and_then(Value::as_str) != Some("restarted") {
                continue;
            }
            let Some(target) = field(args, "name").and_then(Value::as_str) else {
                continue;
            };
            let items = strings(field(handler, "loop"));
            let restarted: Vec<String> = if items.is_empty() {
                vec![unit_name(&resolve(target, vars))]
            } else {
                items
                    .iter()
                    .map(|item| unit_name(&resolve(&target.replace("{{ item }}", item), vars)))
                    .collect()
            };
            handlers.insert(name.to_string(), restarted);
        }
    }
    handlers
}

/// An install that replaces, under an App Version guard, something a unit of the
/// same role is running.
struct Replacement {
    role: String,
    task: String,
    dest: String,
    /// Units left running the old artifact unless a handler restarts them.
    holds: Vec<String>,
    /// Units the task's own `notify` list actually restarts.
    restarts: BTreeSet<String>,
}

impl Replacement {
    fn stale(&self) -> Vec<String> {
        self.holds
            .iter()
            .filter(|unit| !self.restarts.contains(*unit))
            .cloned()
            .collect()
    }
}

fn task_name(task: &Mapping) -> String {
    field(task, "name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_string()
}

/// A guard is on the version-bump path when it names the role's App Version.
fn guards_a_version_bump(role: &str, guards: &[String]) -> bool {
    let token = format!("{role}_version");
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    guards.iter().any(|guard| {
        guard.match_indices(&token).any(|(at, _)| {
            !guard[..at].chars().next_back().is_some_and(word)
                && !guard[at + token.len()..].chars().next().is_some_and(word)
        })
    })
}

fn replacements() -> Vec<Replacement> {
    let mut found = Vec::new();
    for role in all_roles() {
        let vars = defaults(&role);
        let units = units(&role, &vars);
        if units.is_empty() {
            continue;
        }
        let handlers = restart_handlers(&role, &vars);

        for task in every_task(&role) {
            if !guards_a_version_bump(&role, &task.guards) {
                continue;
            }
            for module in INSTALL_MODULES {
                let Some(args) = field(&task.body, module).and_then(Value::as_mapping) else {
                    continue;
                };
                if field(args, "content").is_some() {
                    continue;
                }
                let Some(dest) = field(args, "dest").and_then(Value::as_str) else {
                    continue;
                };
                let dest = resolve(dest, &vars);
                let dest = dest.trim_end_matches('/');
                // The download lands here, the unit runs what was unpacked out
                // of it; a /tmp path is never an artifact (ADR-0027).
                if dest.starts_with("/tmp") || dest.contains("{{") {
                    continue;
                }
                let holds: Vec<String> = units
                    .iter()
                    .filter(|unit| !unit.transient)
                    .filter(|unit| {
                        unit.runs
                            .iter()
                            .any(|path| path == dest || path.starts_with(&format!("{dest}/")))
                    })
                    .map(|unit| unit.name.clone())
                    .collect();
                if holds.is_empty() {
                    continue;
                }
                found.push(Replacement {
                    role: role.clone(),
                    task: task_name(&task.body),
                    dest: dest.to_string(),
                    holds,
                    restarts: strings(field(&task.body, "notify"))
                        .iter()
                        .filter_map(|handler| handlers.get(handler))
                        .flatten()
                        .cloned()
                        .collect(),
                });
            }
        }
    }
    found
}

/// The fence. Every artifact replacement on the version-bump path notifies the
/// restart of everything running out of what it replaced.
#[test]
fn test_a_version_bump_restarts_everything_that_runs_the_artifact() {
    let unrestarted: Vec<String> = replacements()
        .iter()
        .filter(|replacement| !replacement.stale().is_empty())
        .map(|replacement| {
            format!(
                "  {}: `{}` replaces {} and leaves {} running the old one",
                replacement.role,
                replacement.task,
                replacement.dest,
                replacement.stale().join(", ")
            )
        })
        .collect();
    assert!(
        unrestarted.is_empty(),
        "a bump lands the new bytes on a Host where the old ones are already running, and \
         `state: started` no-ops on a unit systemd reports active (#594). `notify:` the restart \
         handler from the task that replaces the artifact (#599):\n{}",
        unrestarted.join("\n")
    );
}

/// Keeps the scan honest. Every one of these roles pins an App Version and
/// installs the artifact its own unit runs, so a change to the model that stops
/// seeing one fails here rather than quietly passing the fence.
#[test]
fn test_the_scan_sees_every_role_that_replaces_what_it_runs() {
    let seen: BTreeSet<String> = replacements()
        .iter()
        .map(|replacement| replacement.role.clone())
        .collect();
    for role in ["bichon", "blocky", "gokapi", "headscale", "paperless"] {
        assert!(
            seen.contains(role),
            "{role} installs the artifact its unit runs and must be scanned; found {seen:?}"
        );
    }
}

/// A `oneshot` run by a timer is not something to restart: it execs the artifact
/// afresh at every activation, so the replacement is live on the next firing.
/// `colporteur` is the case -- it installs a binary its own unit runs, and needs
/// nothing restarted for it.
#[test]
fn test_a_timer_driven_oneshot_is_not_left_running_anything() {
    let vars = defaults("colporteur");
    let unit = units("colporteur", &vars)
        .into_iter()
        .find(|unit| unit.name == "colporteur.service")
        .expect("colporteur deploys colporteur.service");
    assert!(
        unit.transient,
        "colporteur.service is a oneshot; if it stops being one it needs the restart the fence asks for"
    );
    assert!(
        !replacements()
            .iter()
            .any(|replacement| replacement.role == "colporteur"),
        "nothing runs across colporteur's install, so nothing has to be restarted for it"
    );
}

/// The one place a single notify has to cover four units, so the loop expansion
/// the fence depends on is asserted rather than assumed.
#[test]
fn test_a_looped_handler_restarts_every_unit_it_names() {
    let handlers = restart_handlers("paperless", &defaults("paperless"));
    assert_eq!(
        handlers.get("Restart all paperless services"),
        Some(&vec![
            "paperless-webserver.service".to_string(),
            "paperless-consumer.service".to_string(),
            "paperless-task-queue.service".to_string(),
            "paperless-scheduler.service".to_string(),
        ]),
        "paperless replaces one source tree that all four units run out of"
    );
}
