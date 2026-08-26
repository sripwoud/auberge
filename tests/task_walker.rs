//! Fence on the shared ansible task walker the other fences read the tree
//! through.
//!
//! Every fence that asks a question of the repo's ansible tree — which units
//! it installs, which of them declare a restart limit, which paths a service
//! owns, which removals clear a failed state — answers it by walking tasks.
//! The walk is the shared premise underneath all of them, so a walk that
//! quietly stops reaching somewhere does not fail: it shrinks the domain, and
//! every fence over it passes vacuously (#654).
//!
//! These are the walker's own assertions. They are deliberately anchored on
//! shapes the tree actually contains rather than on fixtures, because a
//! fixture proves the walker walks a fixture.

mod common;

use common::{Plays, all_roles, field, role_tasks, tasks_in};
use serde_yaml::Value;

/// The bytes a role's `tasks/` files actually hold, so an empty file can be
/// told apart from a walk that stopped reading.
fn written_tasks(role: &str) -> String {
    let dir = common::role_dir(role).join("tasks");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return String::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<String>()
        .trim()
        .to_string()
}

fn name_of(task: &common::Task) -> String {
    field(&task.body, "name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_string()
}

/// A walk that cannot read a role's `tasks/` directory yields nothing, and
/// nothing is exactly what a role with no tasks yields — so a broken walk is
/// indistinguishable from an empty role unless the two are told apart by what
/// is actually on disk. Every role whose task files hold anything at all must
/// yield at least one task.
///
/// The wireguard role is the reason this is phrased that way and not more
/// simply: its `tasks/main.yml` is a zero-byte file, and has been since the
/// role was added, so the role is a no-op that `apps.yml` still lists.
#[test]
fn test_a_role_whose_task_files_hold_anything_yields_a_task() {
    let barren: Vec<String> = all_roles()
        .into_iter()
        .filter(|role| !written_tasks(role).is_empty())
        .filter(|role| role_tasks(role).is_empty())
        .collect();

    assert!(
        barren.is_empty(),
        "these roles have task files with content in them and yielded no task at \
         all, so every fence's question about them answers vacuously: {}",
        barren.join(", ")
    );
}

/// `block:`/`rescue:`/`always:` carry the task, they are not the task. A walk
/// that stopped at the block would report the wrapper's own keys as a task
/// body and never see the module call inside it.
#[test]
fn test_a_task_inside_a_block_is_yielded_and_the_block_itself_is_not() {
    let tasks = role_tasks("immich");
    let blocks = tasks
        .iter()
        .filter(|task| field(&task.body, "block").is_some())
        .count();
    assert_eq!(
        blocks, 0,
        "a `block:` wrapper was yielded as if it were a task"
    );

    assert!(
        tasks
            .iter()
            .any(|task| field(&task.body, "ansible.builtin.file").is_some()),
        "the immich role's nested tasks did not survive the walk"
    );
}

/// The `when:` standing over a task is what tells a fence whether the task runs
/// on the path it is reasoning about. A guard on an enclosing block applies to
/// everything inside it, so the walk carries it down.
#[test]
fn test_a_task_carries_the_guards_of_every_block_enclosing_it() {
    let guarded: Vec<&common::Task> = all_roles()
        .iter()
        .flat_map(|role| role_tasks(role))
        .filter(|task| !task.guards.is_empty())
        .collect::<Vec<_>>()
        .leak()
        .iter()
        .collect();

    assert!(
        guarded.len() > 100,
        "only {} tasks came back guarded; the `when:` accumulation stopped working",
        guarded.len()
    );

    for task in &guarded {
        let own = common::strings(field(&task.body, "when"));
        for clause in own {
            assert!(
                task.guards.contains(&clause),
                "`{}` carries `when: {clause}` that the walk dropped",
                name_of(task)
            );
        }
    }
}

/// A playbook is a sequence of plays. Its tasks live under `pre_tasks`,
/// `tasks`, `post_tasks` and `handlers`, and are reachable only with
/// `Plays::Descend` — the difference this walker exists to name.
#[test]
fn test_a_plays_tasks_are_reachable_only_by_descending_into_it() {
    // The playbook whose tasks are the reason the flag exists: ADR-0041's
    // radicale removal, 19 tasks deep inside one play.
    let playbook = common::playbooks_dir().join("remove-radicale.yml");

    let descended = tasks_in(&playbook, Plays::Descend);
    let skipped = tasks_in(&playbook, Plays::Skip);

    assert!(
        !descended.is_empty(),
        "{} yielded no task with Plays::Descend",
        playbook.display()
    );
    assert!(
        descended.len() > skipped.len(),
        "descending into the plays of {} found no more than skipping them, so the \
         flag names no difference",
        playbook.display()
    );
    assert!(
        skipped
            .iter()
            .all(|task| field(&task.body, "hosts").is_some()),
        "Plays::Skip yielded something other than the plays themselves"
    );
    assert!(
        descended
            .iter()
            .all(|task| field(&task.body, "hosts").is_none()),
        "Plays::Descend yielded a play as if it were a task"
    );
}

/// A role's `tasks/` file is a bare task sequence with no play in it, so the
/// flag is a no-op there. Asserted rather than assumed: it is what lets a
/// fence over roles pick either value and still be right.
#[test]
fn test_the_flag_changes_nothing_for_a_file_that_holds_no_play() {
    let path = common::role_dir("immich").join("tasks/main.yml");

    let descended = tasks_in(&path, Plays::Descend);
    let skipped = tasks_in(&path, Plays::Skip);

    assert_eq!(descended.len(), skipped.len());
    assert!(!descended.is_empty());
    for (a, b) in descended.iter().zip(skipped.iter()) {
        assert_eq!(a.body, b.body);
        assert_eq!(a.guards, b.guards);
    }
}

/// `runnable_files()` is the widest domain the fences read: every file that can
/// execute a task. A role's handlers and the playbooks themselves are in it
/// precisely because a removal or a restart written there runs like any other.
#[test]
fn test_the_runnable_domain_reaches_handlers_and_playbooks() {
    let files = common::runnable_files();

    let has = |needle: &str| {
        files
            .iter()
            .any(|path| path.to_string_lossy().contains(needle))
    };

    assert!(
        has("/handlers/"),
        "no role handler file is in the runnable domain"
    );
    assert!(has("/playbooks/"), "no playbook is in the runnable domain");
    assert!(
        has("/tasks/"),
        "no role task file is in the runnable domain"
    );
    assert!(
        !files
            .iter()
            .any(|path| path.to_string_lossy().ends_with(".meta.yml")),
        "a Playbook Meta file is in the runnable domain; it carries the CLI's \
         metadata, not tasks"
    );
}

/// `{{ var }}` resolves against the role's own defaults, and anything they do
/// not state is left standing verbatim so an unresolved expression can never
/// compare equal to a real path.
#[test]
fn test_a_default_resolves_and_an_unknown_expression_survives_intact() {
    let vars = common::defaults("immich");
    assert!(
        !vars.is_empty(),
        "the immich role's defaults resolved to nothing"
    );

    assert_eq!(
        common::resolve("{{ nothing_declares_this }}", &vars),
        "{{ nothing_declares_this }}"
    );

    let (key, value) = vars
        .iter()
        .find(|(_, value)| !value.contains("{{"))
        .expect("a role must declare at least one literal default");
    assert_eq!(&common::resolve(&format!("{{{{ {key} }}}}"), &vars), value);
}
