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
//! Underneath the task walk sits the tree walk it reads through, and the fences
//! that never walk a task still ask that half — which roles are there, which
//! files under one can I read:
//!
//! - [`all_roles`] is the role list, directories only.
//! - [`yml_files`] is every `.yml` under a directory, at any depth.
//! - [`playbook_files`] is the playbooks, without the `.meta.yml` sidecars that
//!   run nothing.
//! - [`meta_files`] is the sidecars themselves, for the fences over what an App
//!   declares rather than what it runs.
//!
//! Every fence that carried a copy of *this* walk now reads it through here
//! (#654, #658). `grimmory_role.rs`, `immich_container_dirs.rs` and
//! `probe_after_restart.rs` define a `flatten` too, but theirs are genuinely
//! different walks — a hard-stop assert on a block-level `when:`,
//! `include_tasks` resolution — and belong where they are.
//!
//! Four more fences enumerated the tree with a `read_dir` of their own, two of
//! them without [`all_roles`]'s `is_dir()` filter — the same divergence #658
//! removed from `install_guards.rs`, surviving in files it did not reach. All
//! four read the tree through here now, so the filter has one definition again
//! (#659). What is left of that shape is named where it survives:
//! [`task_name`] on the one accessor that is not a copy of the others, and the
//! three meta enumerators [`meta_files`] has not yet absorbed, listed on it.

// Each of the fences below imports a different subset, so every one of them
// sees the rest as dead. The cost of the blanket allow: a helper that no fence
// uses at all is never flagged either. `tests/task_walker.rs` is the mitigation
// — anything load-bearing has an assertion over it there.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Sequence, Value};

/// The App layer over this walk: which App answers for a role, and the Unit
/// Ownership each App declares. Two fences read it since #720 folded it out
/// of `unit_ownership.rs`.
pub mod apps;
/// The systemd units this walk's tasks install, one layer over it. Five fences
/// re-derived that layer from these primitives before #668 folded it.
pub mod units;

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

/// The repository root. Read here by [`ansible_dir`] and [`relative`], and by
/// `version_annotations.rs` to reach `renovate.json`.
///
/// No longer a route into crate source: `removed_unit_failed_state.rs` reached
/// `src/playbook_meta.rs` through it to mirror the unit-type declaration as
/// text (#656), and imports the `const` instead since #667. `tests/crate_source`
/// carries its own root for the questions still asked of `src/` as text.
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
///
/// `immich_container_dirs.rs` is the sixth and is deliberately not one of them.
/// It reads the name through a local `string_at` path accessor and falls back
/// to a third spelling again, `<unnamed task>`, so folding it is a decision
/// about that file's accessor rather than the mechanical move the other four
/// were. Named here so its absence does not read as coverage.
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

fn collect_yml(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yml(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "yml") {
            out.push(path);
        }
    }
}

/// Every `.yml` file under `dir`, at any depth, sorted.
///
/// A directory that does not exist yields nothing rather than failing: most
/// roles have no `handlers/`, and "which files can I read here?" has an honest
/// empty answer there. Where an empty answer would instead mean the tree moved
/// under the fence, saying so is the caller's job — [`playbook_files`] is the
/// one that has to, and does.
pub fn yml_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_yml(dir, &mut found);
    found.sort();
    found
}

/// The playbooks themselves, without the `.meta.yml` sidecars beside them.
///
/// A Playbook Meta carries the CLI's own metadata — an App's version, its
/// units, its backup order — and runs nothing, so a fence reading plays or
/// tasks must not see it. Three fences want the other half of this directory
/// and each enumerates the metas itself (`version_annotations.rs`,
/// `unit_ownership.rs`, `service_directories.rs`); that is the next leftover,
/// not this one.
///
/// Empty is a hard stop, and it is the reason this is not just a `yml_files`
/// call at each site. Every caller loops over the result, so a directory that
/// moved would leave each of them iterating nothing and passing — the two that
/// now read the tree through here used to spell that guard themselves, as
/// `.expect("ansible/playbooks must exist")`, and would have lost it.
pub fn playbook_files() -> Vec<PathBuf> {
    let mut files = yml_files(&playbooks_dir());
    assert!(
        !files.is_empty(),
        "{} holds no .yml at all; every fence over plays reads this domain",
        relative(&playbooks_dir())
    );
    files.retain(|path| {
        !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".meta.yml"))
    });
    files
}

/// The other half of the playbooks directory: every `<app>.meta.yml`, paired
/// with the App name it declares for, sorted by name.
///
/// [`playbook_files`] filters these out; a fence over the declarations wants
/// exactly them. `version_annotations.rs` and `service_directories.rs` still
/// enumerate the metas with a `read_dir` of their own — folding those two is
/// #658's remit, not this accessor's, and they are named here so their
/// absence does not read as coverage. `unit_ownership.rs` read the metas
/// itself too, until #720 moved its declaration reader into [`apps`], which
/// reads them through here.
///
/// Empty is a hard stop for [`playbook_files`]'s reason: every caller loops the
/// result, so a directory that moved would leave each of them iterating nothing
/// and passing.
pub fn meta_files() -> Vec<(String, PathBuf)> {
    let mut metas: Vec<(String, PathBuf)> = yml_files(&playbooks_dir())
        .into_iter()
        .filter_map(|path| {
            let app = path
                .file_name()?
                .to_str()?
                .strip_suffix(".meta.yml")?
                .to_string();
            Some((app, path))
        })
        .collect();
    assert!(
        !metas.is_empty(),
        "{} holds no .meta.yml at all; every fence over the declarations reads this domain",
        relative(&playbooks_dir())
    );
    metas.sort();
    metas
}

/// Every file in the repository that can run a task: each role's `tasks/` and
/// `handlers/`, at any depth, and the playbooks themselves.
///
/// Walk these with [`Plays::Descend`] — a playbook's tasks are unreachable
/// without it.
pub fn runnable_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for role in all_roles() {
        let dir = role_dir(&role);
        files.extend(yml_files(&dir.join("tasks")));
        files.extend(yml_files(&dir.join("handlers")));
    }
    files.extend(playbook_files());
    files.sort();
    files
}

/// A role's subdirectories whose YAML ansible renders during a deploy.
///
/// `files/` is deliberately absent: ansible copies it byte for byte, so
/// `{{ … }}` in one is literal text and reading it as a reference invents a
/// requirement that does not exist. So is `examples/`, which documents a role
/// for a human and which no deploy renders.
const TEMPLATED_ROLE_DIRS: &[&str] = &["defaults", "handlers", "meta", "tasks", "vars"];

/// One role's templated YAML, at any depth.
pub fn role_yml_files(role: &str) -> Vec<PathBuf> {
    let dir = role_dir(role);
    let mut files: Vec<PathBuf> = TEMPLATED_ROLE_DIRS
        .iter()
        .flat_map(|sub| yml_files(&dir.join(sub)))
        .collect();
    files.sort();
    files
}

/// One role's templates, at any depth.
///
/// Not filtered to `.j2`: ansible templates whatever `ansible.builtin.template`
/// is pointed at, so the extension is a convention and the directory is the
/// fact. Every file there is `.j2` today, and a walk that reads the directory
/// keeps saying something if one is not.
pub fn role_template_files(role: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files(&role_dir(role).join("templates"), &mut files);
    files.sort();
    files
}

fn collect_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, found);
        } else if path.is_file() {
            found.push(path);
        }
    }
}

/// Every YAML ansible renders during a deploy: [`role_yml_files`] for every
/// role, plus the playbooks.
///
/// Wider than [`runnable_files`] by the directories that hold no task but are
/// templated all the same — a `defaults/main.yml` that reads
/// `{{ calibre_subdomain }}` states a requirement as surely as a task does,
/// and is where most of them are stated (#686).
pub fn templated_yml_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = all_roles().iter().flat_map(|r| role_yml_files(r)).collect();
    files.extend(playbook_files());
    files.sort();
    files
}

/// [`role_template_files`] for every role.
pub fn role_templates() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = all_roles()
        .iter()
        .flat_map(|r| role_template_files(r))
        .collect();
    files.sort();
    files
}

/// A YAML file parsed, failing loudly and by repo-relative name.
///
/// Two fences spelled this identically before it was shared (#686) — the shape
/// #654 removed from the task walk, surviving in the parse beneath it.
pub fn parse_yaml(path: &Path) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", relative(path)));
    serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", relative(path)))
}

/// Every name in the Key Registry — the vocabulary a Host's `config.toml` may
/// use, and the only answer to a variable reference that the user supplies.
pub fn registry_keys() -> BTreeSet<String> {
    let path = ansible_dir().join("keys.yml");
    let registry = parse_yaml(&path);
    let keys = registry
        .get("keys")
        .and_then(Value::as_mapping)
        .unwrap_or_else(|| panic!("{} must hold a keys: mapping", relative(&path)));
    let names: BTreeSet<String> = keys
        .keys()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    assert!(!names.is_empty(), "the Key Registry is empty");
    names
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
