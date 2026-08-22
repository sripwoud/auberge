use serde_yaml::{Sequence, Value};
use std::fs;
use std::path::PathBuf;

/// paperless installs by deleting the tree its four units exec and unpacking
/// another one over it, then migrating the database with the new code. Nothing a
/// handler can do reaches inside that: a `notify` flushes at end of play, so the
/// workers execute pre-bump code against a post-bump schema for the whole
/// install, and a `meta: flush_handlers` moved ahead of the migration would
/// restart them into a `venv/` the same block just deleted (#604).
///
/// So the window is the mechanism, and a window is only as good as what it
/// contains. This fences the order the role has to keep: stop the four units,
/// do every destructive thing, start them again.
///
/// The four units, spelled as the role's `loop`s spell them.
const UNITS: &[&str] = &[
    "paperless-webserver",
    "paperless-consumer",
    "paperless-task-queue",
    "paperless-scheduler",
];

/// The guard the stop must run under. Unconditional would take the App down on
/// every deploy, including the ones that change nothing.
const BUMP_GUARD: &str = "paperless_installed_version != paperless_version";

/// Every task that must sit inside the window, in the order the role runs them.
/// Declared rather than derived so that a new destructive task -- or one renamed
/// out of the predicate's reach -- is a failing build and not a silent hole.
const QUIESCED: &[&str] = &[
    "Remove stale source and static trees from previous release",
    "Remove virtual environment from previous release",
    "Extract release tarball",
    "Create Python virtual environment",
    "Install Python dependencies",
    "Download NLTK stopwords data",
    "Download NLTK punkt_tab tokenizer",
    "Run database migrations",
    "Create admin superuser",
];

/// Modules that delete, unpack, or run code out of the install path. `template`
/// is deliberately absent: rendering a config the units read is what the restart
/// handler is for, where these are what no restart can bridge.
const DESTRUCTIVE_MODULES: &[&str] = &[
    "ansible.builtin.unarchive",
    "ansible.builtin.pip",
    "ansible.builtin.command",
    "ansible.builtin.shell",
];

/// What names the source tree and everything under it: the two role variables,
/// and the literal they resolve to. The literal is not redundant -- the role's
/// own defaults spell some of these paths out (`paperless_data_dir:
/// /opt/paperless/data`), so a task written the same way would otherwise be
/// invisible to the predicate below and neither required inside the window nor
/// caught by its equality.
const INSTALL_PATHS: &[&str] = &[
    "paperless_install_path",
    "paperless_src_dir",
    "/opt/paperless",
];

struct Task {
    name: String,
    body: serde_yaml::Mapping,
    guards: Vec<String>,
}

fn field<'a>(task: &'a serde_yaml::Mapping, key: &str) -> Option<&'a Value> {
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

/// The role's tasks in the order ansible runs them, blocks inlined and each
/// task carrying the `when` of every block enclosing it. Order is the whole
/// subject here, which is what separates this from the role-agnostic scans.
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
                name: field(body, "name")
                    .and_then(Value::as_str)
                    .unwrap_or("<unnamed>")
                    .to_string(),
                body: body.clone(),
                guards: scoped,
            });
        }
    }
}

fn tasks() -> Vec<Task> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles/paperless/tasks/main.yml");
    let raw = fs::read_to_string(&path).expect("paperless has a task file");
    let parsed: Sequence =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let mut out = Vec::new();
    flatten(&parsed, &[], &mut out);
    out
}

/// The index of the one task driving all four units to `state`, and the guards
/// it runs under.
fn transition(tasks: &[Task], state: &str) -> (usize, Vec<String>) {
    let matches: Vec<(usize, &Task)> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| {
            let Some(args) =
                field(&task.body, "ansible.builtin.systemd_service").and_then(Value::as_mapping)
            else {
                return false;
            };
            if field(args, "state").and_then(Value::as_str) != Some(state) {
                return false;
            }
            if field(args, "name").and_then(Value::as_str) != Some("{{ item }}") {
                return false;
            }
            let looped = strings(field(&task.body, "loop"));
            UNITS
                .iter()
                .all(|unit| looped.iter().any(|item| item == unit))
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "exactly one task must drive all four units to `{state}`, found {}",
        matches.len()
    );
    let (at, task) = matches[0];
    (at, task.guards.clone())
}

/// Whether a task deletes, unpacks, or runs code out of the install path --
/// the work that cannot happen under a running worker. The whole task is read
/// for the path, not just the module's arguments: the delete that starts the
/// swap names both trees in its `loop` and passes `{{ item }}` as the path.
fn destructive(task: &Task) -> bool {
    let rendered = serde_yaml::to_string(&task.body).unwrap_or_default();
    if !INSTALL_PATHS.iter().any(|var| rendered.contains(var)) {
        return false;
    }
    let deletes = field(&task.body, "ansible.builtin.file")
        .and_then(Value::as_mapping)
        .and_then(|args| field(args, "state"))
        .and_then(Value::as_str)
        == Some("absent");
    deletes
        || DESTRUCTIVE_MODULES
            .iter()
            .any(|module| field(&task.body, module).is_some())
}

#[test]
fn test_the_stop_covers_every_unit_and_only_fires_on_a_bump() {
    let tasks = tasks();
    let (_, guards) = transition(&tasks, "stopped");
    assert!(
        guards.iter().any(|guard| guard == BUMP_GUARD),
        "the stop must be guarded on the bump (`{BUMP_GUARD}`); unconditional takes \
         paperless down on every deploy, and a stricter guard leaves runs that replace \
         the tree without stopping anything"
    );
}

/// What this does *not* assert: that the window exists on every run. Five of the
/// declared tasks -- the venv rebuild, the dependency install, both NLTK
/// downloads, the migration and the superuser create -- sit outside the
/// version-bump block, and the stop is inside it. On a run where the block is
/// skipped they run with nothing stopped. The case that reaches is a hand-deleted
/// `venv/`, which the Installed Version fact does not read (ADR-0027 grounds it
/// on `src/manage.py`): the four units are already crash-looping on a missing
/// `ExecStart` by then, and once pip finishes one can come up on new code ahead
/// of the migration. Narrower than #604 and not what #604 asked for, so it is
/// named here rather than papered over.
#[test]
fn test_every_destructive_task_runs_inside_the_window() {
    let tasks = tasks();
    let (stop, _) = transition(&tasks, "stopped");
    let (start, _) = transition(&tasks, "started");
    assert!(
        stop < start,
        "the units are stopped for the install and started after it"
    );

    let inside: Vec<&str> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| destructive(task))
        .map(|(at, task)| {
            assert!(
                at > stop && at < start,
                "`{}` deletes, unpacks or runs code out of the install path, so on a bump run \
                 it must sit between the stop and the start. Ordered outside them, the four \
                 workers execute pre-bump code across it -- and a `manage.py migrate` outside \
                 them hands those workers a schema their code does not know (#604)",
                task.name
            );
            task.name.as_str()
        })
        .collect();
    assert_eq!(
        inside, QUIESCED,
        "the window's contents are declared, so a destructive task added, renamed, or moved \
         is reviewed rather than assumed covered"
    );
}
