use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Sequence, Value};

fn role_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles/grimmory")
}

/// Tasks in play order with `block`/`rescue`/`always` flattened in place — the
/// grimmory role wraps everything in a single block. A condition on the block
/// itself would AND into every task's guard, which this model does not
/// represent, so meeting one is a hard stop rather than a silent omission.
fn flatten(tasks: &Sequence, out: &mut Vec<Mapping>) {
    for task in tasks {
        let Some(task) = task.as_mapping() else {
            continue;
        };
        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = task.get(Value::from(section)).and_then(Value::as_sequence) {
                assert!(
                    task.get(Value::from("when")).is_none(),
                    "a `when` on the enclosing {section} ANDs into every task guard; \
                     teach this test about it before relying on it"
                );
                flatten(inner, out);
                nested = true;
            }
        }
        if !nested {
            out.push(task.clone());
        }
    }
}

fn role_tasks() -> Vec<Mapping> {
    let raw = fs::read_to_string(role_dir().join("tasks/main.yml")).expect("grimmory tasks");
    let parsed: Sequence = serde_yaml::from_str(&raw).expect("grimmory tasks must parse");
    let mut tasks = Vec::new();
    flatten(&parsed, &mut tasks);
    tasks
}

fn role_defaults() -> Value {
    let raw = fs::read_to_string(role_dir().join("defaults/main.yml")).expect("grimmory defaults");
    serde_yaml::from_str(&raw).expect("grimmory defaults must parse")
}

fn string_at(task: &Mapping, path: &[&str]) -> Option<String> {
    let mut node = &Value::Mapping(task.clone());
    for key in path {
        node = node.get(*key)?;
    }
    node.as_str().map(str::to_string)
}

/// A jinja environment that refuses to silently resolve a variable the caller
/// did not model. Every expression under test is a role template, so an
/// unmodelled variable means the test no longer describes what ansible feeds
/// it — that must fail, not evaluate to a lenient `undefined`.
///
/// `basename` is an ansible filter plugin rather than core jinja, so minijinja
/// has to be taught it; this mirrors the `os.path.basename` it wraps.
fn strict_env() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_filter("basename", |path: &str| {
        path.rsplit('/').next().unwrap_or(path).to_string()
    });
    env
}

/// Resolve a role default the way ansible would for a given pinned version.
fn render_default(key: &str, version: &str) -> String {
    let defaults = role_defaults();
    let template = defaults[key]
        .as_str()
        .unwrap_or_else(|| panic!("the role must define {key}"))
        .to_string();
    let context = minijinja::context! {
        grimmory_install_path => defaults["grimmory_install_path"]
            .as_str()
            .expect("the role must define grimmory_install_path"),
        grimmory_version => version,
    };
    strict_env()
        .render_str(&template, context)
        .unwrap_or_else(|e| panic!("{key} must render: {e}"))
}

/// Index and body of the role's only task invoking `module`.
fn sole_task_using(tasks: &[Mapping], module: &str) -> (usize, Mapping) {
    let mut found = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.contains_key(Value::from(module)));
    let (index, task) = found
        .next()
        .unwrap_or_else(|| panic!("the grimmory role must have a {module} task"));
    assert!(
        found.next().is_none(),
        "{module} is no longer unique in the role; this test can no longer identify the jar download"
    );
    (index, task.clone())
}

/// The jar download, and the stat task whose registered fact its `when` consults.
/// Neither is selected by the path it names, so comparing those paths is a real
/// assertion rather than a restatement of how they were found.
struct JarGuard {
    download_index: usize,
    download_dest: String,
    stat_path: String,
    stat_register: String,
    when: String,
}

fn jar_guard() -> JarGuard {
    let tasks = role_tasks();
    let (download_index, download) = sole_task_using(&tasks, "ansible.builtin.get_url");
    let when = string_at(&download, &["when"]).expect("the jar download must be guarded");

    let mut backing = tasks.iter().enumerate().filter(|(_, task)| {
        task.contains_key(Value::from("ansible.builtin.stat"))
            && string_at(task, &["register"]).is_some_and(|register| when.contains(&register))
    });
    let (stat_index, stat) = backing
        .next()
        .unwrap_or_else(|| panic!("`when: {when}` consults no stat task"));
    assert!(
        backing.next().is_none(),
        "`when: {when}` consults more than one stat; the guard's grounding is ambiguous"
    );
    assert!(
        stat_index < download_index,
        "the stat must run before the download whose guard reads it"
    );

    JarGuard {
        download_index,
        download_dest: string_at(&download, &["ansible.builtin.get_url", "dest"])
            .expect("the download must name a dest"),
        stat_path: string_at(&stat, &["ansible.builtin.stat", "path"])
            .expect("the stat must name a path"),
        stat_register: string_at(&stat, &["register"]).expect("checked above"),
        when: when.clone(),
    }
}

/// Evaluate the download's `when` the way ansible would. The only fact modelled
/// is the pinned jar's stat: with a version-stamped `dest`, whether that path
/// exists is the whole question, and a guard reaching for anything else — a
/// sidecar version marker, say — fails to render.
fn guard_fires(guard: &JarGuard, jar_exists: bool) -> bool {
    let stat = BTreeMap::from([("stat", BTreeMap::from([("exists", jar_exists)]))]);
    let context = BTreeMap::from([(
        guard.stat_register.clone(),
        minijinja::Value::from_serialize(&stat),
    )]);

    let rendered = strict_env()
        .render_str(
            &format!(
                "{{% if {} %}}download{{% else %}}skip{{% endif %}}",
                guard.when
            ),
            context,
        )
        .unwrap_or_else(|e| panic!("`when: {}` must evaluate: {e}", guard.when));
    rendered == "download"
}

/// The find that lists superseded jars, and the removal that consumes it.
struct JarPrune {
    remove_index: usize,
    paths: String,
    patterns: String,
    excludes: Vec<String>,
}

fn jar_prune() -> JarPrune {
    let tasks = role_tasks();
    let (find_index, find) = sole_task_using(&tasks, "ansible.builtin.find");
    let register = string_at(&find, &["register"]).expect("the find must register its matches");

    let mut removals = tasks.iter().enumerate().filter(|(_, task)| {
        string_at(task, &["ansible.builtin.file", "state"]).as_deref() == Some("absent")
            && string_at(task, &["loop"]).is_some_and(|loop_expr| loop_expr.contains(&register))
    });
    let (remove_index, _) = removals
        .next()
        .unwrap_or_else(|| panic!("nothing consumes `{register}`; the find prunes nothing"));
    assert!(
        removals.next().is_none(),
        "more than one removal loops over `{register}`; the prune is ambiguous"
    );
    assert!(
        find_index < remove_index,
        "the find must run before the removal that loops over it"
    );

    let excludes = Value::Mapping(find.clone())["ansible.builtin.find"]["excludes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("the find must exclude the pinned jar from `{register}`"))
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .expect("excludes must be strings")
                .to_string()
        })
        .collect();

    JarPrune {
        remove_index,
        paths: string_at(&find, &["ansible.builtin.find", "paths"])
            .expect("the find must name a path to sweep"),
        patterns: string_at(&find, &["ansible.builtin.find", "patterns"])
            .expect("the find must name a pattern"),
        excludes,
    }
}

#[test]
fn test_a_version_bump_lands_on_a_path_that_cannot_already_exist() {
    let pinned = render_default("grimmory_jar_path", "2.3.0");
    let bumped = render_default("grimmory_jar_path", "2.4.0");
    assert_ne!(
        pinned, bumped,
        "`get_url` with the default `force: false` and no `checksum:` issues a conditional GET \
         against an existing dest (#595); a shared filename makes the new jar's arrival hinge on \
         the release asset's Last-Modified beating the old file's mtime"
    );
}

#[test]
fn test_the_download_fires_exactly_when_the_pinned_jar_is_absent() {
    let guard = jar_guard();
    assert!(
        guard_fires(&guard, false),
        "a version bump — and deleting the jar to recover from a bad release asset (#591) — \
         both surface as a missing dest"
    );
    assert!(
        !guard_fires(&guard, true),
        "a converged install must stay idempotent"
    );
}

#[test]
fn test_the_stat_watches_the_very_path_the_download_writes() {
    let guard = jar_guard();
    assert_eq!(
        guard.stat_path, guard.download_dest,
        "the guard must be grounded in the artifact it protects"
    );
}

#[test]
fn test_systemd_execs_the_jar_the_download_writes() {
    let unit = fs::read_to_string(role_dir().join("templates/grimmory.service.j2"))
        .expect("grimmory unit template");
    let exec_start = unit
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .expect("the unit must have an ExecStart");
    assert!(
        exec_start.contains(&jar_guard().download_dest),
        "the unit execs a different jar than ansible downloads:\n{exec_start}"
    );
}

#[test]
fn test_the_jar_path_has_one_definition() {
    let guard = jar_guard();
    assert_eq!(
        guard.download_dest, "{{ grimmory_jar_path }}",
        "the stat, the download and the unit all resolve the jar through this default"
    );
}

#[test]
fn test_a_redownloaded_jar_is_flagged_for_a_restart() {
    let (_, download) = sole_task_using(&role_tasks(), "ansible.builtin.get_url");
    assert_eq!(
        string_at(&download, &["notify"]).as_deref(),
        Some("Restart grimmory"),
        "a replaced jar only reaches the running process through a restart"
    );
}

#[test]
fn test_the_prune_sweeps_the_directory_the_jar_lands_in() {
    let prune = jar_prune();
    let install_path = render_default("grimmory_install_path", "2.3.0");
    let swept = strict_env()
        .render_str(
            &prune.paths,
            minijinja::context! { grimmory_install_path => &install_path },
        )
        .expect("the find's paths must render");
    assert_eq!(
        swept, install_path,
        "superseded jars accumulate where the download writes them"
    );
    assert_eq!(
        prune.patterns, "*.jar",
        "the pattern must also catch the pre-#595 `app.jar` left on hosts that already deployed"
    );
}

#[test]
fn test_the_prune_spares_the_pinned_jar_at_every_version() {
    let prune = jar_prune();
    for version in ["2.3.0", "2.4.0"] {
        let pinned = render_default("grimmory_jar_path", version);
        let spared: Vec<String> = prune
            .excludes
            .iter()
            .map(|exclude| {
                strict_env()
                    .render_str(
                        exclude,
                        minijinja::context! {
                            grimmory_jar_path => &pinned,
                            grimmory_version => version,
                        },
                    )
                    .expect("each exclude must render")
            })
            .collect();
        let basename = pinned.rsplit('/').next().expect("the jar path has a name");
        assert!(
            spared.iter().any(|exclude| exclude == basename),
            "at {version} the prune deletes the jar the unit execs: \
             sweeping *.jar while sparing {spared:?}"
        );
    }
}

#[test]
fn test_the_prune_runs_after_the_pinned_jar_is_in_place() {
    let guard = jar_guard();
    let prune = jar_prune();
    assert!(
        guard.download_index < prune.remove_index,
        "pruning before the download leaves a window with no jar at all"
    );

    let tasks = role_tasks();
    let (unit_index, _) = tasks
        .iter()
        .enumerate()
        .find(|(_, task)| {
            string_at(task, &["ansible.builtin.template", "dest"])
                .as_deref()
                .is_some_and(|dest| dest.ends_with("/grimmory.service"))
        })
        .expect("the role must deploy a grimmory unit");
    assert!(
        unit_index < prune.remove_index,
        "the unit on disk must already point at the pinned jar before the old ones go, \
         or an aborted play leaves systemd execing a deleted path"
    );
}
