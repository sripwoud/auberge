//! The ansible tree, as the fences read it.
//!
//! Every fence over `ansible/` starts from the same question — what does the
//! repo's own tree say it does? — and answers it by walking tasks: which units
//! a role installs, which of them declare a restart limit, which paths a
//! service owns, which removals clear a failed state. The walk is the shared
//! premise underneath all of those, and a premise that quietly stops reaching
//! somewhere does not fail. It shrinks the domain, and every fence over it goes
//! on passing, vacuously.
//!
//! Six fences kept their own copy of this walk, four of them byte-identical,
//! and the copies had already diverged: one descended into a playbook's own
//! task sections and read role handlers besides, the other five did neither,
//! with nothing anywhere naming the difference. Measured at the point of
//! extraction, that was 648 tasks visible to five of them against 731 to the
//! sixth — 34 reachable only by descending into plays, 49 only by reading a
//! role's handlers. Two fences asking the same question of the same tree got
//! answers 83 tasks apart (#654).
//!
//! So the difference is a parameter with a name, not a property of whichever
//! copy you happen to be reading:
//!
//! - [`Plays`] says whether the walk descends into a play's task sections.
//! - [`tasks_in`] is the narrowest domain: one file, in run order. What a fence
//!   reasoning about ordering needs, since order *across* files is not a fact
//!   the tree states — `install_guards.rs` reads `tasks/main.yml` this way.
//! - [`role_tasks`] is the middle one: one role's whole `tasks/` directory,
//!   for membership questions where order is meaningless.
//! - [`runnable_files`] is the wide one: every file in the repo that can run a
//!   task, role handlers and playbooks included.
//!
//! A fence picks a domain deliberately and says so at the call site. Widening
//! one is then a visible edit to a fence, which is the point.
//!
//! Every fence that carried a copy of *this* walk now reads it through here
//! (#654, #658). `grimmory_role.rs`, `immich_container_dirs.rs` and
//! `probe_after_restart.rs` define a `flatten` too, but theirs are genuinely
//! different walks — a hard-stop assert on a block-level `when:`,
//! `include_tasks` resolution — and belong where they are.
//!
//! What this module does *not* yet answer for everyone: `ingress_gate.rs` and
//! `version_annotations.rs` enumerate `ansible/roles/` with their own
//! `read_dir` and neither carries the `is_dir()` filter [`all_roles`] does, so
//! the divergence #658 removed from `install_guards.rs` survives in files it
//! did not reach (#659). Stating it here rather than leaving the omission to
//! read as coverage.

// Each of the fences below imports a different subset, so every one of them
// sees the rest as dead. The cost of the blanket allow: a helper that no fence
// uses at all is never flagged either. `tests/task_walker.rs` is the mitigation
// — anything load-bearing has an assertion over it there.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Sequence, Value};

/// A task the walk reached, with every `when:` standing over it.
pub struct Task {
    pub body: Mapping,
    /// Every `when:` clause between the file's top level and this task,
    /// outermost first, the task's own included.
    ///
    /// A guard on an enclosing `block:` ANDs into everything inside it — which
    /// is how the bichon and paperless roles gate their installs — so a fence
    /// reasoning about whether a task runs on some path has to see the block's
    /// guard, not just the task's. A play's own `when:` stands over its tasks
    /// the same way under [`Plays::Descend`]. No play in the repo declares one,
    /// so that half is inert today; it is implemented rather than left for the
    /// first one to discover.
    pub guards: Vec<String>,
}

/// Whether a walk descends into a play's own task sections.
///
/// A role's `tasks/` file is a bare task sequence with no play in it, so the
/// two are equivalent there — asserted in `tests/task_walker.rs` rather than
/// assumed, since that is what lets a fence over roles pick either and be
/// right. A playbook is a sequence of plays, and its tasks are reachable only
/// through `pre_tasks`/`tasks`/`post_tasks`/`handlers`: walk one with
/// [`Plays::AsTasks`] and you get the plays, none of their tasks, and a fence
/// that passes because it looked at nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plays {
    /// Yield a play the way any other mapping is yielded, without looking
    /// inside it. Named for what it does rather than for skipping the descent,
    /// because a play walked this way is still handed to the caller as a task.
    /// What a role's task files want.
    AsTasks,
    /// Recurse into `pre_tasks`/`tasks`/`post_tasks`/`handlers` and yield what
    /// is inside, never the play itself. What a playbook wants.
    Descend,
}

/// The repository root. Read by `removed_unit_failed_state.rs` to reach
/// `src/playbook_meta.rs`, whose unit-type declaration it mirrors (#656).
pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ansible_dir() -> PathBuf {
    repo().join("ansible")
}

fn roles_dir() -> PathBuf {
    ansible_dir().join("roles")
}

pub fn playbooks_dir() -> PathBuf {
    ansible_dir().join("playbooks")
}

pub fn role_dir(role: &str) -> PathBuf {
    roles_dir().join(role)
}

/// `path` relative to the repository root, for a message a reader can act on.
pub fn relative(path: &Path) -> String {
    path.strip_prefix(repo())
        .unwrap_or(path)
        .display()
        .to_string()
}

pub fn all_roles() -> Vec<String> {
    let mut roles: Vec<String> = fs::read_dir(roles_dir())
        .expect("ansible/roles must exist")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    roles.sort();
    roles
}

pub fn field<'a>(task: &'a Mapping, key: &str) -> Option<&'a Value> {
    task.get(Value::from(key))
}

/// A field ansible accepts in both the scalar and the sequence form, as a list
/// either way.
pub fn strings(value: Option<&Value>) -> Vec<String> {
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

/// A task's `name:`, or `<unnamed>` for one that declares none — what a failure
/// message needs to point a reader at the task that failed it.
///
/// Borrowed rather than owned so the caller decides: `paperless_quiesce.rs`
/// compares a list of names against a declared one and wants `&str`, while the
/// fences that store a name in a struct call `.to_string()` at that one site.
/// The five copies this replaced had already drifted across two names and two
/// return types with nothing naming the difference (#659).
pub fn task_name(task: &Mapping) -> &str {
    field(task, "name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
}

/// Flattens a task sequence to the tasks that actually call a module.
///
/// `block:`/`rescue:`/`always:` carry tasks, they are not tasks: a wrapper is
/// descended into and never yielded, and the guard standing over it is carried
/// down to everything inside. `plays` decides the same question for a play.
fn flatten(tasks: &Sequence, plays: Plays, inherited: &[String], out: &mut Vec<Task>) {
    for task in tasks {
        let Some(body) = task.as_mapping() else {
            continue;
        };

        let mut scoped = inherited.to_vec();
        scoped.extend(strings(field(body, "when")));

        if plays == Plays::Descend && field(body, "hosts").is_some() {
            for section in ["pre_tasks", "tasks", "post_tasks", "handlers"] {
                if let Some(inner) = field(body, section).and_then(Value::as_sequence) {
                    flatten(inner, plays, &scoped, out);
                }
            }
            continue;
        }

        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = field(body, section).and_then(Value::as_sequence) {
                flatten(inner, plays, &scoped, out);
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

/// Every task one file runs. A file that does not parse is a hard stop: a
/// silently skipped file is a silently narrowed domain.
pub fn tasks_in(path: &Path, plays: Plays) -> Vec<Task> {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", relative(path)));
    let parsed: Sequence =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", relative(path)));
    let mut tasks = Vec::new();
    flatten(&parsed, plays, &[], &mut tasks);
    tasks
}

/// Every task in one role's `tasks/` directory, in file order.
///
/// Order across files is meaningless — this answers which tasks a role has, not
/// what runs when — so an `include_tasks` needs no resolving to be seen: the
/// file it would include is read on its own. A task under a guard is still a
/// task the role runs, so guards narrow nothing here; they ride on the [`Task`]
/// for the fence to weigh.
pub fn role_tasks(role: &str) -> Vec<Task> {
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
    files
        .iter()
        .flat_map(|file| tasks_in(file, Plays::AsTasks))
        .collect()
}

fn yml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            yml_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "yml") {
            out.push(path);
        }
    }
}

/// Every file in the repository that can run a task: each role's `tasks/` and
/// `handlers/`, at any depth, and the playbooks themselves. `.meta.yml` files
/// carry the CLI's own metadata, not tasks.
///
/// Walk these with [`Plays::Descend`] — a playbook's tasks are unreachable
/// without it.
pub fn runnable_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for role in fs::read_dir(roles_dir())
        .expect("ansible/roles must exist")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
    {
        yml_files(&role.join("tasks"), &mut files);
        yml_files(&role.join("handlers"), &mut files);
    }
    yml_files(&playbooks_dir(), &mut files);
    files.retain(|path| {
        !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".meta.yml"))
    });
    files.sort();
    files
}

/// A role's scalar defaults, as a substitution table — which is where every
/// path a unit runs and every path an install writes is stated (ADR-0027).
///
/// Structured values are left out: nothing that reads this resolves into a list
/// or a mapping.
pub fn defaults(role: &str) -> BTreeMap<String, String> {
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
/// changing. Anything else — a filter, a register's field, an App Version
/// injected as an extra_var — is left standing verbatim, which is the point: a
/// `dest` and an `ExecStart` that resolve through the same default arrive here
/// as the same text, so grimmory's `…/grimmory-{{ grimmory_version }}.jar`
/// compares equal on both sides without the version's value being knowable from
/// the repo at all. An expression nothing resolves therefore cannot compare
/// equal to a real path or unit name either.
///
/// Rendering with minijinja instead would substitute an undefined name with the
/// empty string and silently compare a wrong path. `immich_container_dirs.rs`
/// does render, under a strict environment that errors on the undefined name
/// rather than emptying it — the other half of the same argument.
pub fn resolve(raw: &str, vars: &BTreeMap<String, String>) -> String {
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
