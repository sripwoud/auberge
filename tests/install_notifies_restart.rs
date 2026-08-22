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
/// Scope is the version-bump path: the install decides what lands by naming the
/// App Version, `<role>_version` (ADR-0017, the convention
/// `version_annotations.rs` enforces). A role says that in one of three places,
/// one per install regime, and all three are read here -- a `when` guard
/// comparing the Installed Version, the `version:` ref of a `git` checkout, or
/// the dest itself where the artifact's path carries the version.
///
/// A restart is one of two ways to satisfy this. The other is to stop what runs
/// out of the artifact for the length of the install, which is what a role has
/// to do when the install is destructive enough that no restart can bridge it:
/// paperless deletes the venv its four units exec before unpacking the new tree,
/// and migrates the database in between, so a handler flushed at end of play
/// bounds the stale window without closing it (#604). A unit stopped under
/// guards no stricter than the replacement's is not left running anything, and
/// needs no notify.
///
/// What is out of scope is the *missing*-artifact path, guarded on a bare `stat`
/// with no version anywhere: an absent `ExecStart` target means the unit is
/// already dead, so the `state: started` further down revives it and no handler
/// is needed. A versioned dest is not that case, however much it looks like it —
/// the path that does not exist yet is the *new* one, and the unit is alive on
/// the old one, which is why grimmory is in scope here.
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
/// changing. Anything else -- a filter, a register's field, an App Version
/// injected as an extra_var -- is left standing verbatim, which is the point:
/// a dest and an `ExecStart` that resolve through the same default arrive here
/// as the same text, so grimmory's `…/grimmory-{{ grimmory_version }}.jar`
/// compares equal on both sides without the version's value being knowable
/// from the repo at all. Rendering with minijinja instead would substitute an
/// undefined name with the empty string and silently compare a wrong path.
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
    /// Whether the process keeps running whatever it started with, which is
    /// what makes a replacement on disk something to restart. False for a
    /// `oneshot` without `RemainAfterExit`: it execs its artifact afresh at
    /// every activation, so the next timer firing picks the replacement up on
    /// its own. `immich` is why `Type=oneshot` alone is not the test --
    /// `RemainAfterExit` keeps the containers it started alive.
    holds_the_artifact: bool,
}

/// A command line split into arguments, on whitespace *outside* `{{ }}` only.
/// Splitting on every space would tear a path holding an unresolved variable in
/// half at `grimmory-{{ grimmory_version }}.jar`, and the half left over
/// matches no dest.
fn arguments(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                depth += 1;
                current.push(c);
                current.push(chars.next().expect("peeked"));
            }
            '}' if chars.peek() == Some(&'}') => {
                depth = depth.saturating_sub(1);
                current.push(c);
                current.push(chars.next().expect("peeked"));
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn directive_paths(body: &str, vars: &BTreeMap<String, String>) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            ["ExecStart=", "WorkingDirectory="]
                .iter()
                .find_map(|directive| line.strip_prefix(directive))
        })
        .flat_map(|value| {
            arguments(&resolve(value.trim(), vars))
                .into_iter()
                .filter(|token| token.starts_with('/'))
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
            // A drop-in refines a unit rather than being one, and the units
            // these refine (navidrome's, icecast's, caddy's) are installed by
            // apt, not templated by the role that budgets their memory.
            if !dest.starts_with("/etc/systemd/system")
                || dest
                    .trim_start_matches("/etc/systemd/system/")
                    .contains('/')
            {
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
                    holds_the_artifact: !body.contains("Type=oneshot")
                        || body.contains("RemainAfterExit"),
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

/// The units the role stops, each with the guards its stop runs under and its
/// position in the role's task order. Position matters: a stop that runs *after*
/// the replacement leaves the old artifact serving for exactly as long as one
/// that never runs, and a stop in a block's `always:` flattens after the block
/// it follows.
fn stopped_units(role: &str, vars: &BTreeMap<String, String>) -> Vec<(String, Vec<String>, usize)> {
    let mut stopped = Vec::new();
    for (at, task) in every_task(role).into_iter().enumerate() {
        for module in SERVICE_MODULES {
            let Some(args) = field(&task.body, module).and_then(Value::as_mapping) else {
                continue;
            };
            if field(args, "state").and_then(Value::as_str) != Some("stopped") {
                continue;
            }
            let Some(target) = field(args, "name").and_then(Value::as_str) else {
                continue;
            };
            let items = strings(field(&task.body, "loop"));
            let names: Vec<String> = if items.is_empty() {
                vec![unit_name(&resolve(target, vars))]
            } else {
                items
                    .iter()
                    .map(|item| unit_name(&resolve(&target.replace("{{ item }}", item), vars)))
                    .collect()
            };
            for name in names {
                stopped.push((name, task.guards.clone(), at));
            }
        }
    }
    stopped
}

/// Whether `unit` is stopped ahead of the replacement at `before`, under guards
/// the replacement's guards already imply -- so no run that replaces the artifact
/// leaves that unit on the old one. Three ways to fail this, all checked: a stop
/// carrying a guard the replacement does not can be skipped on a run that still
/// replaces; a stop positioned after the replacement quiesces nothing; and a
/// stop naming another unit says nothing about this one.
fn quiesced(
    stops: &[(String, Vec<String>, usize)],
    unit: &str,
    guards: &[String],
    before: usize,
) -> bool {
    stops.iter().any(|(stopped, under, at)| {
        stopped == unit && *at < before && under.iter().all(|guard| guards.contains(guard))
    })
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

/// Whether a string names the role's App Version, as a whole word rather than a
/// substring -- `blocky_lego_version` is a Tool Version and must not read as
/// `blocky_version`.
fn names_the_app_version(role: &str, text: &str) -> bool {
    let token = format!("{role}_version");
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(&token).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(word)
            && !text[at + token.len()..].chars().next().is_some_and(word)
    })
}

/// Whether an install decides what lands by naming the App Version -- the
/// version-bump path, in whichever of the three regimes the role installs by:
///
/// - a `when` guard comparing the Installed Version to the pinned one, which is
///   marker-plus-stat and artifact-read (bichon, blocky, gokapi, headscale,
///   paperless);
/// - the `version:` ref of a `git` checkout, where the module's own parameter is
///   the guard: git moves the tree to that ref and reports changed exactly when
///   it moved something (freshrss, tgtg);
/// - the dest, where the artifact's path carries the version, so the bump lands
///   on a path that cannot already exist (grimmory, #597).
fn on_the_version_bump_path(role: &str, guards: &[String], args: &Mapping, dest: &str) -> bool {
    guards
        .iter()
        .any(|guard| names_the_app_version(role, guard))
        || field(args, "version")
            .and_then(Value::as_str)
            .is_some_and(|reference| names_the_app_version(role, reference))
        || names_the_app_version(role, dest)
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
        let stops = stopped_units(&role, &vars);

        for (at, task) in every_task(&role).into_iter().enumerate() {
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
                if dest.starts_with("/tmp") {
                    continue;
                }
                if !on_the_version_bump_path(&role, &task.guards, args, dest) {
                    continue;
                }
                let holds: Vec<String> = units
                    .iter()
                    .filter(|unit| unit.holds_the_artifact)
                    .filter(|unit| !quiesced(&stops, &unit.name, &task.guards, at))
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

/// Every role the scan reaches. Asserted as equality, not membership: the fence
/// above is only as good as what it looks at, and a role that installs what its
/// own unit runs while going unseen would pass it for free. Adding a role here
/// is what subjects it to the fence, and a role dropping off the list is a
/// coverage regression rather than a passing suite.
///
/// The roles that install by App Version and are *not* on this list are the
/// ones the model cannot prove anything about; those carry declared notify
/// edges in `DECLARED_ROLES` below, and the split between the two lists is
/// itself asserted.
const REPLACING_ROLES: &[&str] = &[
    "bichon",
    "blocky",
    "freshrss",
    "gokapi",
    "grimmory",
    "headscale",
    "tgtg",
];

#[test]
fn test_the_scan_sees_every_role_that_replaces_what_it_runs() {
    let seen: BTreeSet<String> = replacements()
        .iter()
        .map(|replacement| replacement.role.clone())
        .collect();
    let declared: BTreeSet<String> = REPLACING_ROLES.iter().map(|r| r.to_string()).collect();
    assert_eq!(
        seen, declared,
        "a role that installs, under an App Version, the artifact its own unit runs \
         must be declared here -- that is what puts it under the fence"
    );
}

/// Every role with an install on the version-bump path, whether or not the
/// model could follow its dest into a unit. No `/tmp` skip here: for baikal the
/// one `INSTALL_MODULES` task on that path is the download its shell install
/// unpacks, and the download is what marks the role as installing by version.
fn version_bump_installers() -> BTreeSet<String> {
    let mut installers = BTreeSet::new();
    for role in all_roles() {
        let vars = defaults(&role);
        for task in every_task(&role) {
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
                if on_the_version_bump_path(&role, &task.guards, args, dest.trim_end_matches('/')) {
                    installers.insert(role.clone());
                }
            }
        }
    }
    installers
}

/// A version-bump installer the dest→unit model does not fence, with the notify
/// edges a human vouches for in its place.
///
/// The model's verdict on these roles cannot be trusted on its own, because
/// clearing a role correctly and clearing it wrongly look identical from
/// inside the repo: what runs baikal's release is apt's php-fpm, whose unit no
/// role templates, so the model finds nothing running across the install --
/// exactly what it finds for colporteur, where nothing genuinely does. So the
/// verdict is declared per role and fenced two ways: the set of roles needing
/// a declaration is computed and matched exactly, and every declared edge is
/// asserted to exist and to reach a handler that actually restarts.
struct DeclaredRole {
    role: &'static str,
    /// Why the model cannot follow this role's install into what runs it.
    why: &'static str,
    /// The `(task, handler)` notify edges that must exist. Empty declares the
    /// model's clearance correct: nothing runs across the install.
    notifies: &'static [(&'static str, &'static str)],
}

const DECLARED_ROLES: &[DeclaredRole] = &[
    DeclaredRole {
        role: "baikal",
        why: "its release is served by the system's php-fpm, installed by apt; the role \
              templates only its two oneshot sync timers, so there is no unit to follow \
              the install into",
        notifies: &[(
            "Install Baikal release (replaces Core, html, vendor; keeps Specific and config)",
            "Restart baikal php-fpm",
        )],
    },
    // Cleared, not covered: the model's finding that nothing runs across the
    // install is the correct one here, and
    // `test_a_timer_driven_oneshot_is_not_left_running_anything` asserts it.
    DeclaredRole {
        role: "colporteur",
        why: "a timer-driven oneshot execs the artifact afresh at every activation, so \
              the replacement is live on the next firing with nothing to restart",
        notifies: &[],
    },
    DeclaredRole {
        role: "hermes",
        why: "what its unit execs is a venv built by a `command`, which names no dest to \
              follow, and the unit is a systemd user unit under ~/.config/systemd/user",
        notifies: &[("Install hermes-agent into venv", "Restart hermes gateway")],
    },
    DeclaredRole {
        role: "navidrome",
        why: "the deb lands what a unit owned by apt runs -- the role templates only a \
              memory drop-in, which the model excludes as refining a unit rather than \
              being one, and the installing module is `apt`, which it does not follow",
        notifies: &[("Install Navidrome from .deb package", "Restart navidrome")],
    },
    // Cleared, not covered: nothing runs across paperless's install because the
    // role stops what does for its length, and `tests/paperless_quiesce.rs`
    // asserts that window spans every destructive task in it (#604).
    DeclaredRole {
        role: "paperless",
        why: "it stops the four units that run out of the source tree before deleting it, \
              and starts them after the migration, so the model's finding that nothing is \
              left on the old tree is the correct one -- there is no restart to demand",
        notifies: &[],
    },
    DeclaredRole {
        role: "yourls",
        why: "served by the system's php-fpm, installed by apt; the role templates no \
              unit at all",
        notifies: &[
            ("Clone YOURLS repository", "Restart yourls php-fpm"),
            ("Update YOURLS repository", "Restart yourls php-fpm"),
        ],
    },
];

/// The declared complement of the fence. Exact equality in both directions:
/// a new role installing by App Version out of the model's reach fails until
/// a human classifies it here, and a declaration the model has since learned
/// to reach -- or whose role is gone -- fails until it is removed, so the list
/// can neither silently grow nor silently rot.
#[test]
fn test_an_install_the_model_cannot_prove_is_declared_and_wired() {
    let seen: BTreeSet<String> = replacements()
        .iter()
        .map(|replacement| replacement.role.clone())
        .collect();
    let unproven: BTreeSet<String> = version_bump_installers()
        .difference(&seen)
        .cloned()
        .collect();
    let declared: BTreeSet<String> = DECLARED_ROLES
        .iter()
        .map(|declaration| declaration.role.to_string())
        .collect();
    assert_eq!(
        unproven, declared,
        "every version-bump installer the dest→unit model does not fence must be \
         declared in DECLARED_ROLES, either with the notify edges that cover it or \
         with an explicit empty clearance"
    );

    for declaration in DECLARED_ROLES {
        let tasks = every_task(declaration.role);
        let handlers = restart_handlers(declaration.role, &defaults(declaration.role));
        for (task, handler) in declaration.notifies {
            let carrying = tasks
                .iter()
                .find(|candidate| task_name(&candidate.body) == *task)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: `{task}` is declared to notify `{handler}` but no task of \
                         that name exists",
                        declaration.role
                    )
                });
            assert!(
                strings(field(&carrying.body, "notify"))
                    .iter()
                    .any(|notified| notified == handler),
                "{}: `{task}` must notify `{handler}` -- {}",
                declaration.role,
                declaration.why
            );
            assert!(
                handlers.contains_key(*handler),
                "{}: `{handler}` must be a handler that restarts (`state: restarted`); \
                 anything less is the no-op this family of bugs is made of",
                declaration.role
            );
        }
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
        !unit.holds_the_artifact,
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
/// the fence depends on is asserted rather than assumed. paperless no longer
/// reaches it from its extract, but its two config templates still do, and both
/// have to land on all four.
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
        "paperless renders one config that all four units read"
    );
}

/// The quiesced case. paperless deletes the venv its four units exec and
/// migrates the database before anything could be restarted into the new tree,
/// so the stop is what satisfies the fence and the notify would be a second
/// mechanism claiming the same job (#604).
#[test]
fn test_a_quiesced_install_leaves_nothing_running_the_old_artifact() {
    let vars = defaults("paperless");
    let stops = stopped_units("paperless", &vars);
    let bump = vec!["paperless_installed_version != paperless_version".to_string()];
    let extract = every_task("paperless")
        .iter()
        .position(|task| task_name(&task.body) == "Extract release tarball")
        .expect("paperless unpacks a release tarball");
    for unit in [
        "paperless-webserver.service",
        "paperless-consumer.service",
        "paperless-task-queue.service",
        "paperless-scheduler.service",
    ] {
        assert!(
            quiesced(&stops, unit, &bump, extract),
            "{unit} must be stopped ahead of the extract, under the version-bump guard \
             and nothing stricter"
        );
    }
    assert!(
        !replacements()
            .iter()
            .any(|replacement| replacement.role == "paperless"),
        "nothing runs across paperless's install, so nothing has to be restarted for it"
    );
}
