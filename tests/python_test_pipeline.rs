use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `run` string of a `mise.toml` task.
fn mise_task(name: &str) -> String {
    let path = repo_root().join("mise.toml");
    let manifest: toml::Value =
        toml::from_str(&read(&path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    manifest
        .get("tasks")
        .and_then(|tasks| tasks.get(name))
        .and_then(|task| task.get("run"))
        .and_then(|run| run.as_str())
        .unwrap_or_else(|| panic!("mise.toml: [tasks.{name}] must declare a string `run`"))
        .to_string()
}

/// The directories a pytest invocation collects from: the positional arguments
/// after the executable. A token following a flag is that flag's value, not a
/// target — `-p no:cacheprovider` names no directory. Every remaining token has
/// to be one, so a mistyped target fails here rather than narrowing the reach
/// the suites are checked against, and a command with no target at all fails
/// too.
fn pytest_targets(run: &str) -> Vec<PathBuf> {
    let tokens: Vec<&str> = run.split_whitespace().collect();
    let executable = tokens
        .iter()
        .enumerate()
        .position(|(i, token)| *token == "pytest" && (i == 0 || tokens[i - 1] != "--with"))
        .unwrap_or_else(|| panic!("`{run}` does not invoke pytest"));

    let targets: Vec<PathBuf> = (executable + 1..tokens.len())
        .filter(|i| !tokens[*i].starts_with('-') && !tokens[i - 1].starts_with('-'))
        .map(|i| {
            assert!(
                repo_root().join(tokens[i]).is_dir(),
                "`{run}` collects from {}, which is not a directory in this repo",
                tokens[i]
            );
            PathBuf::from(tokens[i])
        })
        .collect();

    assert!(!targets.is_empty(), "`{run}` names no directory to collect");
    targets
}

/// The packages a `uv run` command provisions with `--with`.
fn with_packages(run: &str) -> BTreeSet<String> {
    run.split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|pair| pair[0] == "--with")
        .map(|pair| pair[1].to_string())
        .collect()
}

/// Every `test_*.py` in the repository, repo-relative.
fn python_test_files() -> Vec<PathBuf> {
    fn walk(dir: &PathBuf, found: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if path.is_dir() {
                walk(&path, found);
            } else if name.starts_with("test_") && name.ends_with(".py") {
                found.push(path.strip_prefix(repo_root()).unwrap().to_path_buf());
            }
        }
    }

    let mut found = Vec::new();
    walk(&repo_root(), &mut found);
    found
}

/// Every package an `ansible.builtin.pip` task in a role installs, across every
/// file under `tasks/` and at any depth of block nesting, in both the sequence
/// and the scalar `name:` forms. Asserted non-empty: a role whose pip task
/// moved to an included file, or whose list shrank to one scalar, would
/// otherwise report nothing to provision and pass every caller vacuously.
fn role_pip_packages(role: &str) -> BTreeSet<String> {
    fn collect(value: &serde_yaml::Value, found: &mut BTreeSet<String>) {
        match value {
            serde_yaml::Value::Mapping(map) => {
                for (key, child) in map {
                    if key.as_str() == Some("ansible.builtin.pip")
                        && let Some(name) = child.get("name")
                    {
                        match name {
                            serde_yaml::Value::Sequence(names) => found
                                .extend(names.iter().filter_map(|n| n.as_str()).map(String::from)),
                            serde_yaml::Value::String(name) => {
                                found.insert(name.clone());
                            }
                            _ => {}
                        }
                    }
                    collect(child, found);
                }
            }
            serde_yaml::Value::Sequence(items) => items.iter().for_each(|v| collect(v, found)),
            _ => {}
        }
    }

    let tasks = repo_root().join("ansible/roles").join(role).join("tasks");
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(&tasks)
        .unwrap_or_else(|e| panic!("{}: {e}", tasks.display()))
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let parsed: serde_yaml::Value = serde_yaml::from_str(&read(&path))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        collect(&parsed, &mut found);
    }

    assert!(
        !found.is_empty(),
        "no `ansible.builtin.pip` package found under {} — the scan stopped matching \
         what the role writes, so anything comparing against it passes vacuously",
        tasks.display()
    );
    found
}

/// The `run` commands of a workflow job's steps that execute unconditionally.
fn unconditional_step_commands(workflow: &PathBuf, job: &str) -> Vec<String> {
    let parsed: serde_yaml::Value = serde_yaml::from_str(&read(workflow))
        .unwrap_or_else(|e| panic!("{}: {e}", workflow.display()));

    parsed
        .get("jobs")
        .and_then(|jobs| jobs.get(job))
        .and_then(|job| job.get("steps"))
        .and_then(|steps| steps.as_sequence())
        .unwrap_or_else(|| panic!("{}: job `{job}` has no steps", workflow.display()))
        .iter()
        .filter(|step| step.get("if").is_none())
        .filter_map(|step| step.get("run").and_then(|run| run.as_str()))
        .map(|run| run.trim().to_string())
        .collect()
}

/// A suite nothing invokes proves nothing. The two baikal suites sat outside
/// `mise r test` and outside CI while the scripts they cover shipped three
/// silent data-loss bugs, each exiting 0 (#616 dropped blob-stored birthdays,
/// #637 dropped the 10 whose `BDAY` omits the year, #484 shifted floating
/// iCloud times by the Berlin offset). A new suite landing in a directory the
/// task does not collect from would be just as invisible (#643).
#[test]
fn test_every_python_suite_is_collected_by_the_task() {
    let run = mise_task("test-python");
    let targets = pytest_targets(&run);

    let unreached: Vec<String> = python_test_files()
        .into_iter()
        .filter(|suite| !targets.iter().any(|target| suite.starts_with(target)))
        .map(|suite| format!("\n    {}", suite.display()))
        .collect();

    assert!(
        unreached.is_empty(),
        "mise.toml [tasks.test-python] collects from {targets:?}, which leaves these suites \
         running nowhere — add their directory to the task:{}",
        unreached.concat()
    );
}

/// The `--with` list exists to reproduce the environment the Host runs the
/// script under: `baikal-busy-sync.py` imports from the venv the role
/// provisions, so a package added there and not here turns a real import error
/// into a test-time `ModuleNotFoundError` at best, and a green suite covering
/// an unimportable script at worst.
#[test]
fn test_the_task_provisions_what_the_role_installs() {
    let provisioned = with_packages(&mise_task("test-python"));

    let missing: Vec<String> = role_pip_packages("baikal")
        .into_iter()
        .filter(|package| !provisioned.contains(package))
        .map(|package| format!("\n    {package}"))
        .collect();

    assert!(
        missing.is_empty(),
        "the baikal role installs these into the busy feed venv, and \
         mise.toml [tasks.test-python] does not provision them — add `--with <package>`:{}",
        missing.concat()
    );
}

/// `jdx/mise-action`'s `mise_toml:` input overwrites the repo's `mise.toml` in
/// the workspace, so the `check` job cannot call a task this repo defines — it
/// has to repeat the command. The copies then drift silently, which is how
/// `test-shell` came to list five scripts locally and run three in CI (#649).
/// Matched against the parsed step rather than the file's text, so a command
/// that survives only inside a comment, or behind an `if:`, does not count as
/// CI running it.
#[test]
fn test_ci_runs_the_same_command_as_the_task() {
    let run = mise_task("test-python");
    let workflow = repo_root().join(".github/workflows/master.yml");

    assert!(
        unconditional_step_commands(&workflow, "check").contains(&run),
        "no unconditional step of the `check` job in {} runs the command \
         mise.toml [tasks.test-python] declares, so the python suites pass locally \
         and are absent from CI. Expected a step whose `run` is:\n    {run}",
        workflow.display()
    );
}

/// The guards above only run in the `_test` job, which is gated on a
/// changed-files list. `mise.toml` has to be in that list or the one PR shape
/// that breaks the wiring — an edit to the task and nothing else — skips the
/// job asserting it, and merges green against CI's now-stale hard-coded copy.
#[test]
fn test_the_gate_covers_the_file_the_task_lives_in() {
    let workflow = repo_root().join(".github/workflows/master.yml");
    let parsed: serde_yaml::Value = serde_yaml::from_str(&read(&workflow))
        .unwrap_or_else(|e| panic!("{}: {e}", workflow.display()));

    let patterns = parsed
        .get("jobs")
        .and_then(|jobs| jobs.get("changed-files"))
        .and_then(|job| job.get("steps"))
        .and_then(|steps| steps.as_sequence())
        .unwrap_or_else(|| panic!("{}: no changed-files job", workflow.display()))
        .iter()
        .find_map(|step| step.get("with").and_then(|with| with.get("files")))
        .and_then(|files| files.as_str())
        .unwrap_or_else(|| panic!("{}: changed-files declares no `files`", workflow.display()));

    let covered: BTreeSet<&str> = patterns.split_whitespace().collect();
    for path in ["mise.toml", "tests/**/*.rs"] {
        assert!(
            covered.contains(path),
            "{} gates the job that runs this file's assertions on {covered:?}, which omits \
             `{path}` — a PR touching only that path would skip them",
            workflow.display()
        );
    }
}
