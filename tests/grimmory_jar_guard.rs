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

fn string_at(task: &Mapping, path: &[&str]) -> Option<String> {
    let mut node = &Value::Mapping(task.clone());
    for key in path {
        node = node.get(*key)?;
    }
    node.as_str().map(str::to_string)
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
        download_dest: string_at(&download, &["ansible.builtin.get_url", "dest"])
            .expect("the download must name a dest"),
        stat_path: string_at(&stat, &["ansible.builtin.stat", "path"])
            .expect("the stat must name a path"),
        stat_register: string_at(&stat, &["register"]).expect("checked above"),
        when: when.clone(),
    }
}

/// Evaluate the download's `when` the way ansible would: a jinja expression over
/// the sidecar version marker and the jar's stat.
fn guard_fires(guard: &JarGuard, installed_version: &str, version: &str, jar_exists: bool) -> bool {
    let stat = BTreeMap::from([("stat", BTreeMap::from([("exists", jar_exists)]))]);
    let context = BTreeMap::from([
        (
            "grimmory_installed_version".to_string(),
            minijinja::Value::from(installed_version),
        ),
        (
            "grimmory_version".to_string(),
            minijinja::Value::from(version),
        ),
        (
            guard.stat_register.clone(),
            minijinja::Value::from_serialize(&stat),
        ),
    ]);

    let rendered = minijinja::Environment::new()
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

#[test]
fn test_missing_jar_is_redownloaded_even_when_the_version_marker_matches() {
    assert!(
        guard_fires(&jar_guard(), "2.3.0", "2.3.0", false),
        "deleting app.jar is the recovery path for a bad release asset (#591); \
         a sidecar version marker must not veto the re-download"
    );
}

#[test]
fn test_present_jar_at_the_pinned_version_is_left_alone() {
    assert!(
        !guard_fires(&jar_guard(), "2.3.0", "2.3.0", true),
        "a converged install must stay idempotent"
    );
}

#[test]
fn test_version_bump_redownloads_a_present_jar() {
    assert!(guard_fires(&jar_guard(), "2.3.0", "2.4.0", true));
}

#[test]
fn test_fresh_host_downloads_the_jar() {
    assert!(guard_fires(&jar_guard(), "", "2.3.0", false));
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
    let defaults = fs::read_to_string(role_dir().join("defaults/main.yml")).expect("defaults");
    let parsed: Value = serde_yaml::from_str(&defaults).expect("defaults must parse");
    assert_eq!(
        parsed["grimmory_jar_path"].as_str(),
        Some("{{ grimmory_install_path }}/app.jar"),
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
