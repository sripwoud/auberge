//! Fleet-wide guard on the failed state a removed unit leaves behind.
//!
//! `failed` is a terminal state systemd only leaves on `reset-failed`, on a
//! successful `start`, or when the unit is garbage-collected. `systemctl stop`
//! is not one of them — a stopped unit still reports `ActiveState=failed` —
//! and neither is deleting the unit file underneath it. Removing a service
//! that happens to be failed therefore strands the failure as a `not-found
//! failed` entry that survives reboots and every later deploy: auberge carried
//! one for apache2 from bootstrap on 2026-08-21 until #635 cleared it by hand,
//! on a host with no apache2 installed (#636).
//!
//! `systemctl --failed` is the fleet's first-look health check, and it is only
//! worth reading if everything in it is real. One permanent phantom is enough
//! to stop anyone reading it, so every site that makes a unit disappear has to
//! clear that unit's state on the way out.
//!
//! Which units a Debian package ships is not something this repo can compute,
//! so this is ADR-0028's declared regime: the packages the fleet purges are
//! computed from the tree, the units each one ships are declared in
//! `PURGED_PACKAGES`, and a package the tree purges that the table does not
//! classify fails the build.

use std::collections::{BTreeMap, BTreeSet};

use auberge::playbook_meta::UNIT_TYPE_SUFFIXES;
use serde_yaml::{Mapping, Value};

mod common;

use common::{Plays, field, relative, runnable_files, strings, task_name, tasks_in};

/// A package the fleet purges, and the units it installs.
///
/// `@` templates are deliberately absent: a template unit is never itself
/// instantiated, so it has no state to latch, and `reset-failed` on a bare
/// template name addresses nothing. Each entry was read off the package with
/// `dpkg -c`, not inferred from the name.
struct PurgedPackage {
    package: &'static str,
    /// Plain units the package installs, by unit name.
    units: &'static [&'static str],
    why: &'static str,
}

const PURGED_PACKAGES: &[PurgedPackage] = &[
    PurgedPackage {
        package: "apache2",
        units: &["apache2.service", "apache-htcacheclean.service"],
        why: "ships apache2.service and apache-htcacheclean.service under \
              /usr/lib/systemd/system, alongside an `@` template of each",
    },
    PurgedPackage {
        package: "apache2-bin",
        units: &[],
        why: "modules and the httpd binary only",
    },
    PurgedPackage {
        package: "apache2-data",
        units: &[],
        why: "icons, error documents and default site content only",
    },
    PurgedPackage {
        package: "apache2-utils",
        units: &[],
        why: "htpasswd and friends only - which is why the apt role can keep it \
              installed while purging the server around it, and why the \
              radicale playbook's purge of it strands nothing",
    },
    PurgedPackage {
        package: "libapache2-mod-php8.4",
        units: &[],
        why: "an apache module, which runs inside the server's own process",
    },
    PurgedPackage {
        package: "radicale",
        units: &["radicale.service"],
        why: "ships exactly one unit under /usr/lib/systemd/system",
    },
];

/// Directories systemd reads unit files from, plus a user's own. A drop-in
/// lands in a `<unit>.d/` directory under one of these and refines a unit
/// installed by something else, so it never matches a unit suffix.
const UNIT_DIRS: &[&str] = &[
    "/etc/systemd/system",
    "/usr/lib/systemd/system",
    "/lib/systemd/system",
];

/// The unit a path names, if the path is a plain unit file in a directory
/// systemd loads units from.
///
/// A name that still carries an unresolved jinja expression is a hard stop,
/// for the reason `tests/shutdown_exit_status.rs` gives about the installing
/// side: an unresolved name fails the suffix test and drops out of the domain
/// unseen, which is the one way a removal could enter the tree without
/// entering the fence. An unresolved *directory* is not that case and is not
/// caught here - it fails the directory test instead, and is recorded as a
/// limit in ADR-0041 rather than guessed at.
///
/// The suffix test reads the crate's own [`UNIT_TYPE_SUFFIXES`], so a unit type
/// this fence does not know is not a thing that can happen. It could before:
/// the file carried its own copy of the table, and a type missing from the copy
/// is a removal that fails this test, leaves the domain without a word, and
/// keeps `test_the_scan_still_sees_every_removal_site` green — that test pins
/// only what the scan found. Which is how the fence shipped admitting five of
/// systemd's eleven unit types (#653), with a green build saying otherwise.
/// #656 patched it with a second copy plus a scraper that read the declaration
/// out of `src/playbook_meta.rs` as *text*; #667 deleted both for the import.
fn unit_at<'a>(path: &'a str, file: &str, task: &str) -> Option<&'a str> {
    let (dir, name) = path.rsplit_once('/')?;
    let known = UNIT_DIRS.contains(&dir) || dir.ends_with("/.config/systemd/user");
    if !known {
        return None;
    }
    assert!(
        !name.contains("{{"),
        "{file}: `{task}` removes `{path}` from a systemd unit directory under a \
         name that does not resolve; teach this test how to expand it before \
         relying on it"
    );
    UNIT_TYPE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
        .then_some(name)
}

fn declared_for(package: &str) -> Option<&'static PurgedPackage> {
    PURGED_PACKAGES
        .iter()
        .find(|entry| entry.package == package)
}

/// A unit some task makes disappear, and the task that does it.
struct Removal {
    file: String,
    task: String,
    unit: String,
    why: String,
}

/// A `systemctl reset-failed` invocation, already expanded over its `loop`.
struct Reset {
    file: String,
    task: String,
    /// Everything the invocation names, verbatim. One entry is the only form
    /// that keeps the change signal; the rest exist so the test can say so.
    targets: Vec<String>,
}

fn packages_purged(task: &Mapping, file: &str) -> Vec<String> {
    let Some(args) = field(task, "ansible.builtin.apt").and_then(Value::as_mapping) else {
        return Vec::new();
    };
    if field(args, "state").and_then(Value::as_str) != Some("absent") {
        return Vec::new();
    }
    // `name`'s aliases, all three of which ansible accepts.
    let mut names = Vec::new();
    for key in ["name", "package", "pkg"] {
        names.extend(strings(field(args, key)));
    }
    for name in &names {
        assert!(
            !name.contains("{{"),
            "{file}: `{}` purges `{name}`, a package name that does not resolve; \
             teach this test how to expand it before relying on it",
            task_name(task)
        );
    }
    names
}

fn unit_files_removed(task: &Mapping, file: &str) -> Vec<String> {
    let Some(args) = field(task, "ansible.builtin.file").and_then(Value::as_mapping) else {
        return Vec::new();
    };
    if field(args, "state").and_then(Value::as_str) != Some("absent") {
        return Vec::new();
    }
    // `path`'s aliases. Every unit file the fleet installs is written under
    // `dest:`, so the alias is the likelier form for a removal to reach for.
    ["path", "dest", "name"]
        .iter()
        .find_map(|key| field(args, key).and_then(Value::as_str))
        .and_then(|path| unit_at(path, file, task_name(task)))
        .map(|unit| vec![unit.to_string()])
        .unwrap_or_default()
}

fn reset_in(task: &Mapping, file: &str) -> Option<Reset> {
    let command = field(task, "ansible.builtin.command").and_then(Value::as_str)?;
    let tail = command.split_once("reset-failed")?.1.trim();
    let items = strings(field(task, "loop"));
    let expansions: Vec<String> = if tail.contains("{{") {
        assert!(
            !items.is_empty(),
            "{file}: `{}` templates its reset-failed target but has no literal \
             `loop` this test can expand",
            task_name(task)
        );
        items
            .iter()
            .map(|item| tail.replace("{{ item }}", item))
            .collect()
    } else {
        vec![tail.to_string()]
    };
    for expansion in &expansions {
        assert!(
            !expansion.contains("{{"),
            "{file}: `{}` resets `{expansion}`, which still carries an \
             unresolved expression",
            task_name(task)
        );
    }
    Some(Reset {
        file: file.to_string(),
        task: task_name(task).to_string(),
        targets: expansions,
    })
}

/// One walk of the tree, carrying everything the assertions below read.
struct Scan {
    removals: Vec<Removal>,
    resets: Vec<Reset>,
    purged: BTreeSet<String>,
}

fn scan() -> Scan {
    let mut removals = Vec::new();
    let mut resets = Vec::new();
    let mut purged = BTreeSet::new();
    for path in runnable_files() {
        let file = relative(&path);
        // Plays::Descend, because the radicale removal this fence exists for
        // lives in a playbook: 19 tasks inside one play, none of them reachable
        // without descending into it.
        for task in tasks_in(&path, Plays::Descend) {
            for package in packages_purged(&task.body, &file) {
                purged.insert(package.clone());
                let Some(declared) = declared_for(&package) else {
                    panic!(
                        "{file}: `{}` purges `{package}`, which PURGED_PACKAGES does \
                         not classify. Read the units it ships off the package \
                         (`dpkg -c`) and add an entry - an unclassified purge is a \
                         unit that may latch `failed` with nobody watching",
                        task_name(&task.body)
                    );
                };
                for unit in declared.units {
                    removals.push(Removal {
                        file: file.clone(),
                        task: task_name(&task.body).to_string(),
                        unit: (*unit).to_string(),
                        why: format!("the `{package}` package it purges {}", declared.why),
                    });
                }
            }
            for unit in unit_files_removed(&task.body, &file) {
                removals.push(Removal {
                    file: file.clone(),
                    task: task_name(&task.body).to_string(),
                    why: format!("it deletes `{unit}`'s unit file"),
                    unit,
                });
            }
            if let Some(reset) = reset_in(&task.body, &file) {
                resets.push(reset);
            }
        }
    }
    Scan {
        removals,
        resets,
        purged,
    }
}

/// Every unit the fleet removes clears its own state on the way out. Scoped to
/// the file that removes it: the reset has to run wherever the removal does,
/// and a reset in some other role would not.
#[test]
fn test_every_removed_unit_clears_its_failed_state() {
    let scan = scan();
    let mut cleared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for reset in &scan.resets {
        cleared
            .entry(reset.file.clone())
            .or_default()
            .extend(reset.targets.iter().cloned());
    }
    for removal in &scan.removals {
        assert!(
            cleared
                .get(&removal.file)
                .is_some_and(|units| units.contains(&removal.unit)),
            "{}: `{}` removes `{}` because {}, and nothing in the file clears \
             that unit's failed state. Stopping a unit does not clear it and \
             deleting its unit file does not either, so a run that found it \
             failed strands a not-found entry in `systemctl --failed` forever. \
             Add `systemctl reset-failed {}` after the removal",
            removal.file,
            removal.task,
            removal.unit,
            removal.why,
            removal.unit
        );
    }
}

/// One unit per invocation, because neither shorter form survives contact with
/// `changed_when: <result>.rc == 0`. Measured: `reset-failed a b` returns 1
/// when any argument is not loaded even where it cleared another, so a real
/// change reports as unchanged; `reset-failed 'apache2*'` returns 0 whether it
/// matched a latch or nothing, so every deploy reports as changed. There is no
/// module for the verb and ansible-lint rejects reading the state with
/// `is-failed`, so rc is the only signal there is.
#[test]
fn test_every_reset_names_exactly_one_unit() {
    for reset in &scan().resets {
        for target in &reset.targets {
            let tokens: Vec<&str> = target.split_whitespace().collect();
            assert_eq!(
                tokens.len(),
                1,
                "{}: `{}` resets {tokens:?} in one invocation; rc then answers \
                 for the whole set and `changed_when` stops meaning anything. \
                 Loop over the units instead",
                reset.file,
                reset.task
            );
            assert!(
                !target.contains(['*', '?', '[']),
                "{}: `{}` resets the glob `{target}`, which exits 0 whether it \
                 cleared a latch or matched nothing. Name the units",
                reset.file,
                reset.task
            );
        }
    }
}

/// Declared -> computed. A package nothing purges any more is a claim about
/// Debian that nobody checks, and the units listed against it would go stale
/// unnoticed.
#[test]
fn test_every_declared_package_is_still_purged() {
    let purged = scan().purged;
    for entry in PURGED_PACKAGES {
        assert!(
            purged.contains(entry.package),
            "PURGED_PACKAGES classifies `{}` but nothing in the tree purges it; \
             drop the entry with the last task that did",
            entry.package
        );
    }
}

/// The reach of the scan, pinned as a set. A count stays green when one
/// removal replaces another and cannot name which moved.
#[test]
fn test_the_scan_still_sees_every_removal_site() {
    let seen: BTreeSet<String> = scan()
        .removals
        .iter()
        .map(|removal| format!("{}::{}", removal.file, removal.unit))
        .collect();
    let expected: BTreeSet<String> = [
        "ansible/playbooks/remove-radicale.yml::radicale.service",
        "ansible/roles/apt/tasks/main.yml::apache-htcacheclean.service",
        "ansible/roles/apt/tasks/main.yml::apache2.service",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        seen.difference(&expected).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "the scan found unit removals this test does not list; every removed \
         unit strands a failed state, so add them"
    );
    assert_eq!(
        expected.difference(&seen).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "this test lists removals the scan no longer finds; either the removal \
         is gone or the scan stopped seeing it - the second is the dangerous one"
    );
}
