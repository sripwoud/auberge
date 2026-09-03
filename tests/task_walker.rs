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

use common::{Plays, all_roles, field, playbook_files, role_tasks, task_name, tasks_in, yml_files};

/// The bytes a role's `tasks/` files actually hold, so an empty file can be told
/// apart from a walk that stopped reading.
///
/// Nothing here is allowed to fail quietly. A swallowed `read_dir` or
/// `read_to_string` would report every role as empty, which is precisely the
/// answer that makes the caller's assertion vacuous.
fn written_tasks(role: &str) -> String {
    let dir = common::role_dir(role).join("tasks");
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
    entries
        .map(|entry| entry.expect("a directory entry must be readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
        .map(|path| {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// A walk that cannot read a role's `tasks/` directory yields nothing, and
/// nothing is exactly what a role with no tasks yields — so a broken walk is
/// indistinguishable from an empty role unless the two are told apart by what
/// is actually on disk. Every role whose task files hold anything at all must
/// yield at least one task.
///
/// The count is checked against every role on disk rather than against all
/// but one: a role whose task files hold nothing deploys nothing, and fails
/// here instead of being budgeted for. The allowance this replaces was sized
/// for wireguard alone (#665).
#[test]
fn test_a_role_whose_task_files_hold_anything_yields_a_task() {
    let roles = all_roles();
    let written: Vec<String> = roles
        .iter()
        .filter(|role| !written_tasks(role).is_empty())
        .cloned()
        .collect();

    // Without this the test passes by finding nothing to check: read every role
    // as empty and `barren` is empty too.
    assert_eq!(
        written.len(),
        roles.len(),
        "a role under ansible/roles has empty task files; either the read side \
         of this test has broken or a role that deploys nothing has been added"
    );

    let barren: Vec<&String> = written
        .iter()
        .filter(|role| role_tasks(role).is_empty())
        .collect();

    assert!(
        barren.is_empty(),
        "these roles have task files with content in them and yielded no task at \
         all, so every fence's question about them answers vacuously: {barren:?}"
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
///
/// Both halves are asserted, because either alone passes on a broken walk: a
/// walk that dropped inherited guards still reports every task's own `when:`,
/// and a walk that reported only inherited ones still returns something.
#[test]
fn test_a_task_carries_the_guards_of_every_block_enclosing_it() {
    let guarded: Vec<common::Task> = all_roles()
        .iter()
        .flat_map(|role| role_tasks(role))
        .filter(|task| !task.guards.is_empty())
        .collect();

    assert!(
        !guarded.is_empty(),
        "no task came back guarded at all; the `when:` accumulation stopped working"
    );

    let mut inherited = 0;
    for task in &guarded {
        let own = common::strings(field(&task.body, "when"));
        for clause in &own {
            assert!(
                task.guards.contains(clause),
                "`{}` carries `when: {clause}` that the walk dropped",
                task_name(&task.body)
            );
        }
        if task.guards.len() > own.len() {
            inherited += 1;
        }
    }

    assert!(
        inherited > 0,
        "{} tasks are guarded and not one of them carries a guard from an \
         enclosing block, so nothing proves the walk accumulates rather than \
         just reading each task's own `when:`",
        guarded.len()
    );
}

/// A playbook is a sequence of plays. Its tasks live under `pre_tasks`,
/// `tasks`, `post_tasks` and `handlers`, and are reachable only with
/// `Plays::Descend` — the difference this walker exists to name.
///
/// Every playbook is checked rather than one named example. ADR-0041's
/// `remove-radicale.yml` was the file that motivated the flag, with 19 tasks
/// inside one play, but it was a transitional removal playbook: naming it here
/// would have made deleting it panic this fence, which is exactly what its
/// removal in #820 did without touching this test.
#[test]
fn test_a_plays_tasks_are_reachable_only_by_descending_into_it() {
    let playbooks: Vec<std::path::PathBuf> = common::runnable_files()
        .into_iter()
        .filter(|path| path.starts_with(common::playbooks_dir()))
        .collect();
    assert!(
        !playbooks.is_empty(),
        "no playbook is in the runnable domain"
    );

    let mut deeper = Vec::new();
    for playbook in &playbooks {
        let at = common::relative(playbook);
        let descended = tasks_in(playbook, Plays::Descend);
        let as_tasks = tasks_in(playbook, Plays::AsTasks);

        assert!(
            as_tasks
                .iter()
                .all(|task| field(&task.body, "hosts").is_some()),
            "{at}: Plays::AsTasks yielded something that is not one of the plays"
        );
        assert!(
            descended
                .iter()
                .all(|task| field(&task.body, "hosts").is_none()),
            "{at}: Plays::Descend yielded a play as if it were a task"
        );

        if descended.len() > as_tasks.len() {
            deeper.push(at);
        }
    }

    assert!(
        !deeper.is_empty(),
        "not one of the {} playbooks yielded more with Plays::Descend than \
         without it, so the flag names no difference anywhere",
        playbooks.len()
    );
}

/// A role's `tasks/` file is a bare task sequence with no play in it, so the
/// flag is a no-op there. Asserted rather than assumed: it is what lets a
/// fence over roles pick either value and still be right.
#[test]
fn test_the_flag_changes_nothing_for_a_file_that_holds_no_play() {
    let path = common::role_dir("immich").join("tasks/main.yml");

    let descended = tasks_in(&path, Plays::Descend);
    let as_tasks = tasks_in(&path, Plays::AsTasks);

    assert!(
        !descended.is_empty(),
        "{} yielded no task at all, so the two walks agree on nothing",
        path.display()
    );
    assert_eq!(
        descended.len(),
        as_tasks.len(),
        "{} yielded a different number of tasks under each value of the flag",
        path.display()
    );
    for (descended, as_task) in descended.iter().zip(as_tasks.iter()) {
        assert_eq!(
            descended.body,
            as_task.body,
            "{}: the two walks disagree on a task body",
            path.display()
        );
        assert_eq!(
            descended.guards,
            as_task.guards,
            "{}: the two walks disagree on `{}`'s guards",
            path.display(),
            task_name(&descended.body)
        );
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

/// The tree walk underneath the task walk. A fence that asks "which files can I
/// read here?" and gets a short answer narrows its domain exactly as a task walk
/// that stops descending does, and just as quietly (#659).
#[test]
fn test_the_file_walk_reaches_a_role_and_tolerates_an_absent_directory() {
    let ssh = yml_files(&common::role_dir("ssh").join("tasks"));
    assert!(
        ssh.len() > 1,
        "the ssh role has more than one task file; the walk found {}",
        ssh.len()
    );

    assert!(
        yml_files(&common::role_dir("ssh").join("no-such-directory")).is_empty(),
        "a directory that does not exist yields nothing rather than failing"
    );
}

/// The one assertion here that a fixture serves better than the tree.
///
/// Everything else in this file is anchored on shapes `ansible/` actually
/// contains, because a fixture only proves the walker walks a fixture. Ordering
/// is the exception: it is a postcondition of the walker, not a fact about the
/// tree, and `read_dir` returns whatever the filesystem holds. This checkout was
/// written by git in index order, so every directory in `ansible/` happens to
/// come back sorted already — drop the `sort()` and the suite stays green. A
/// directory built in the opposite order is what makes the claim falsifiable.
///
/// Whole path, not basename: a nested file sorts under its own directory
/// rather than among the files beside it, so `nested/bravo.yml` follows
/// `mike.yml` instead of `alpha.yml`.
#[test]
fn test_the_file_walk_reports_in_path_order_whatever_the_filesystem_says() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["zulu.yml", "mike.yml", "alpha.yml"] {
        std::fs::write(dir.path().join(name), "[]").unwrap();
    }
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/bravo.yml"), "[]").unwrap();

    let found: Vec<String> = yml_files(dir.path())
        .iter()
        .map(|path| {
            path.strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert_eq!(
        found,
        vec!["alpha.yml", "mike.yml", "nested/bravo.yml", "zulu.yml"],
        "the walk must report in whole-path order, not in whatever order `read_dir` hands back"
    );
}

/// `playbook_files` exists to keep the CLI's `.meta.yml` sidecars out of a walk
/// over plays. Asserting only that none come back would pass just as well if the
/// directory held none, so the exclusion is measured against what is actually
/// beside them.
#[test]
fn test_the_playbook_walk_excludes_the_meta_sidecars_beside_them() {
    let plays = playbook_files();
    let every = yml_files(&common::playbooks_dir());

    // Non-emptiness is `playbook_files`'s own hard stop, so there is nothing
    // left to assert about it here.
    assert!(
        every.len() > plays.len(),
        "`.meta.yml` sidecars sit beside the playbooks, so excluding them has to drop something; {} files, {} playbooks",
        every.len(),
        plays.len()
    );
    for path in &plays {
        assert!(
            !path.to_string_lossy().ends_with(".meta.yml"),
            "{} is a Playbook Meta and runs nothing",
            common::relative(path)
        );
    }
}

/// The templated domain is wider than the runnable one by the directories that
/// hold no task but are rendered anyway, and narrower than the tree by the two
/// that are never rendered at all (#686).
#[test]
fn test_the_templated_domain_adds_defaults_and_drops_what_is_never_rendered() {
    let templated = common::templated_yml_files();
    let runnable = common::runnable_files();

    let has = |files: &[std::path::PathBuf], needle: &str| {
        files
            .iter()
            .any(|path| path.to_string_lossy().contains(needle))
    };

    assert!(
        has(&templated, "/defaults/"),
        "no role defaults file is in the templated domain, and that is where \
         most variable references are written"
    );
    assert!(
        has(&templated, "/meta/"),
        "no role meta file is in the templated domain, and that is where a role \
         declares the roles it drags in"
    );
    assert!(
        !has(&runnable, "/defaults/"),
        "the runnable domain reaches defaults now; the two domains no longer \
         differ and one of them is redundant"
    );
    assert!(
        !has(&templated, "/files/"),
        "a role's files/ is in the templated domain; ansible copies those byte \
         for byte, so reading `{{ … }}` in one invents a requirement"
    );
    assert!(
        !has(&templated, "/examples/"),
        "a role's examples/ is in the templated domain; no deploy renders it"
    );
    for path in &runnable {
        assert!(
            templated.contains(path),
            "{} runs tasks but is outside the templated domain, which has to be \
             the wider of the two",
            common::relative(path)
        );
    }
}

/// The per-role walks and the whole-tree walks are the same domain, so a fence
/// reading one role at a time cannot be reading less than one reading all of
/// them — which is the difference the scoped sweep in
/// `variable_answerability.rs` depends on being exact.
#[test]
fn test_the_per_role_walks_partition_the_whole_tree() {
    let mut from_roles: Vec<std::path::PathBuf> = all_roles()
        .iter()
        .flat_map(|role| common::role_yml_files(role))
        .collect();
    from_roles.extend(playbook_files());
    from_roles.sort();
    assert_eq!(
        from_roles,
        common::templated_yml_files(),
        "walking roles one at a time and walking the tree disagree about the \
         templated domain"
    );

    let mut templates: Vec<std::path::PathBuf> = all_roles()
        .iter()
        .flat_map(|role| common::role_template_files(role))
        .collect();
    templates.sort();
    assert_eq!(
        templates,
        common::role_templates(),
        "walking roles one at a time and walking the tree disagree about the \
         template domain"
    );
}

/// Templates are enumerated by their directory, not their extension.
#[test]
fn test_the_template_walk_reads_the_directory_not_the_suffix() {
    let templates = common::role_templates();

    assert!(
        !templates.is_empty(),
        "no role template is in the template domain"
    );
    for path in &templates {
        assert!(
            path.to_string_lossy().contains("/templates/"),
            "{} is in the template domain but not under a templates/ directory",
            common::relative(path)
        );
        assert!(
            path.is_file(),
            "{} is in the template domain but is not a file",
            common::relative(path)
        );
    }

    // Every template in the tree is `.j2` today, which is the convention the
    // walk deliberately does not enforce. Asserting the convention holds is
    // what makes a break in it visible here rather than as a file nothing
    // notices in `variable_answerability.rs`.
    let strays: Vec<String> = templates
        .iter()
        .filter(|path| !path.extension().is_some_and(|ext| ext == "j2"))
        .map(|path| common::relative(path))
        .collect();
    assert!(
        strays.is_empty(),
        "template(s) without a .j2 suffix: {}. The walk reads them, so nothing \
         is unscanned — but the convention moved and this is the notice",
        strays.join(", ")
    );
}

/// The Key Registry read, and the parse underneath it, both fail loudly.
#[test]
fn test_the_registry_walk_reads_the_registry_and_the_parse_says_which_file() {
    let keys = common::registry_keys();
    assert!(
        keys.contains("domain"),
        "the Key Registry does not hold `domain`, which every App's Caddy site \
         is built from; the registry read is looking at the wrong file"
    );

    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.yml");
    std::fs::write(&broken, "a: [1,\n").unwrap();
    let failure = std::panic::catch_unwind(|| common::parse_yaml(&broken))
        .expect_err("unparseable YAML must panic rather than yield an empty document");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or("");
    assert!(
        message.contains("broken.yml") && message.contains("must parse"),
        "a parse failure has to name the file it read; got {message:?}"
    );
}
