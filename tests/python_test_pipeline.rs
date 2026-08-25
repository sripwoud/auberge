use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `run` string of a `mise.toml` task.
fn mise_task(name: &str) -> String {
    let path = repo_root().join("mise.toml");
    let content = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let manifest: toml::Value =
        toml::from_str(&content).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    manifest
        .get("tasks")
        .and_then(|tasks| tasks.get(name))
        .and_then(|task| task.get("run"))
        .and_then(|run| run.as_str())
        .unwrap_or_else(|| panic!("mise.toml: [tasks.{name}] must declare a string `run`"))
        .to_string()
}

/// The directories a pytest invocation collects from: its trailing positional
/// arguments. Read from the executable rightwards rather than by testing which
/// tokens happen to name a path — a flag value that resolved to a directory
/// would otherwise widen the reach the suites are asserted against, and a
/// mistyped target would narrow it to nothing without failing.
fn pytest_targets(run: &str) -> Vec<PathBuf> {
    let tokens: Vec<&str> = run.split_whitespace().collect();
    let executable = tokens
        .iter()
        .enumerate()
        .position(|(i, token)| *token == "pytest" && (i == 0 || tokens[i - 1] != "--with"))
        .unwrap_or_else(|| panic!("`{run}` does not invoke pytest"));

    tokens[executable + 1..]
        .iter()
        .filter(|token| !token.starts_with('-'))
        .map(|token| {
            assert!(
                repo_root().join(token).is_dir(),
                "`{run}` collects from {token}, which is not a directory in this repo"
            );
            PathBuf::from(token)
        })
        .collect()
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

/// Every package name listed by an `ansible.builtin.pip` task in a role,
/// wherever it sits in the task tree.
fn role_pip_packages(role: &str) -> BTreeSet<String> {
    fn collect(value: &serde_yaml::Value, found: &mut BTreeSet<String>) {
        match value {
            serde_yaml::Value::Mapping(map) => {
                for (key, child) in map {
                    if key.as_str() == Some("ansible.builtin.pip")
                        && let Some(names) = child.get("name").and_then(|n| n.as_sequence())
                    {
                        found.extend(names.iter().filter_map(|n| n.as_str()).map(String::from));
                    }
                    collect(child, found);
                }
            }
            serde_yaml::Value::Sequence(items) => items.iter().for_each(|v| collect(v, found)),
            _ => {}
        }
    }

    let path = repo_root()
        .join("ansible/roles")
        .join(role)
        .join("tasks/main.yml");
    let content = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let tasks: serde_yaml::Value =
        serde_yaml::from_str(&content).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let mut found = BTreeSet::new();
    collect(&tasks, &mut found);
    found
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

/// The `--with` list exists to reproduce the interpreter the Host runs the
/// script under: `baikal-busy-sync.py` imports from the venv the role
/// provisions, so a package added there and not here turns a real import
/// error into a test-time `ModuleNotFoundError` at best, and a green suite
/// covering an unimportable script at worst.
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
/// `test-shell` came to list five scripts locally and run three in CI. Pin
/// them together instead of trusting them to stay equal.
#[test]
fn test_ci_runs_the_same_command_as_the_task() {
    let run = mise_task("test-python");
    let workflow = repo_root().join(".github/workflows/master.yml");
    let content =
        fs::read_to_string(&workflow).unwrap_or_else(|e| panic!("{}: {e}", workflow.display()));

    assert!(
        content.contains(&run),
        "{} does not run the command mise.toml [tasks.test-python] declares, so the \
         python suites pass locally and are absent from CI. Expected to find:\n    {run}",
        workflow.display()
    );
}
