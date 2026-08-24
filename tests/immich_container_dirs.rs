//! Structural guards on the immich role's container-owned directories.
//!
//! A host path the vendored compose stack bind-mounts into a container has its
//! interior owned by that container's entrypoint — postgres chowns `PGDATA` to
//! its internal uid and chmods it `00700` on every start, not only on the
//! first. Ansible may create such a path; it may not maintain it. Re-asserting
//! `root:root 0700` on the top directory strips the container uid's search
//! permission on the walk to every relation file it does not already hold open,
//! and nothing closes that window until the next container start (#630,
//! ADR-0036).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Sequence, Value};

fn role_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles/immich")
}

/// Tasks in play order with `block`/`rescue`/`always` flattened in place — the
/// immich role wraps everything in a single block. A condition on the block
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
    let raw = fs::read_to_string(role_dir().join("tasks/main.yml")).expect("immich tasks");
    let parsed: Sequence = serde_yaml::from_str(&raw).expect("immich tasks must parse");
    let mut tasks = Vec::new();
    flatten(&parsed, &mut tasks);
    tasks
}

fn role_defaults() -> Value {
    let raw = fs::read_to_string(role_dir().join("defaults/main.yml")).expect("immich defaults");
    serde_yaml::from_str(&raw).expect("immich defaults must parse")
}

fn string_at(task: &Mapping, path: &[&str]) -> Option<String> {
    let mut node = &Value::Mapping(task.clone());
    for key in path {
        node = node.get(*key)?;
    }
    node.as_str().map(str::to_string)
}

fn task_name(task: &Mapping) -> String {
    string_at(task, &["name"]).unwrap_or_else(|| "<unnamed task>".to_string())
}

/// A jinja environment that refuses to silently resolve a variable the caller
/// did not model. Every expression under test is a role template, so an
/// unmodelled variable means the test no longer describes what ansible feeds
/// it — that must fail, not evaluate to a lenient `undefined`.
fn strict_env() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env
}

fn render<S: serde::Serialize>(expr: &str, context: S) -> String {
    strict_env()
        .render_str(expr, context)
        .unwrap_or_else(|e| panic!("`{expr}` must render: {e}"))
}

/// The role's defaults as a jinja context, so a role expression resolves the way
/// ansible would resolve it.
fn defaults_context() -> BTreeMap<String, minijinja::Value> {
    role_defaults()
        .as_mapping()
        .expect("immich defaults must be a mapping")
        .iter()
        .filter_map(|(key, value)| {
            Some((
                key.as_str()?.to_string(),
                minijinja::Value::from(value.as_str()?),
            ))
        })
        .collect()
}

/// Render a role expression against the role's defaults. Ansible templates
/// recursively, so a default naming another default resolves all the way down;
/// render to a fixpoint rather than one level, and refuse to spin on a
/// self-reference.
fn render_in_role(expr: &str, extra: BTreeMap<String, minijinja::Value>) -> String {
    let mut context = defaults_context();
    context.extend(extra);
    let mut current = render(expr, &context);
    for _ in 0..8 {
        if !current.contains("{{") {
            return current;
        }
        current = render(&current, &context);
    }
    panic!("`{expr}` never resolves to a literal; it may reference itself")
}

/// The concrete path a compose `${VAR}` names, read through the env template the
/// role renders beside the compose file and then through the role's defaults.
fn resolve_env_var(var: &str) -> String {
    let raw = fs::read_to_string(role_dir().join("templates/immich.env.j2"))
        .expect("immich env template");
    let assignment = raw
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == var)
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_else(|| {
            panic!("the compose file mounts `{var}`, which the env template never sets")
        });

    let role_var = assignment
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or_else(|| {
            panic!(
                "`{var}={assignment}` is not a bare role variable; teach this test to \
                 resolve it before relying on this fence"
            )
        });

    let defaults = role_defaults();
    let declared = defaults
        .get(role_var)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the role must define `{role_var}`, which `{var}` resolves to"));
    render_in_role(declared, BTreeMap::new())
}

/// The host-side source of every bind mount the vendored compose file declares,
/// resolved into a concrete path. `${VAR}` indirections resolve through the env
/// template into a role variable and then through the role's defaults; literal
/// host paths pass straight through; a named volume is not a host path and drops
/// out, but only after being matched against the file's own `volumes:` block.
///
/// A source this model cannot resolve is a hard stop. Dropping one silently
/// would narrow the fence to whatever it happened to parse, which is the
/// failure mode it exists to prevent.
fn compose_bind_sources() -> BTreeSet<String> {
    let raw = fs::read_to_string(role_dir().join("files/docker-compose.yml"))
        .expect("immich compose file");
    let compose: Value = serde_yaml::from_str(&raw).expect("immich compose must parse");

    let named_volumes: BTreeSet<String> = compose
        .get("volumes")
        .and_then(Value::as_mapping)
        .map(|volumes| {
            volumes
                .keys()
                .map(|key| {
                    key.as_str()
                        .expect("a compose volume name must be a string")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();

    let services = compose
        .get("services")
        .and_then(Value::as_mapping)
        .expect("the compose file must declare services");

    let mut sources = BTreeSet::new();
    for (service_name, service) in services {
        let service_name = service_name.as_str().unwrap_or("<unnamed service>");
        let Some(volumes) = service.get("volumes").and_then(Value::as_sequence) else {
            continue;
        };
        for entry in volumes {
            let entry = entry.as_str().unwrap_or_else(|| {
                panic!(
                    "service `{service_name}` declares a long-form volume; teach this \
                     test to read it before relying on this fence"
                )
            });
            let source = entry.split(':').next().expect("split yields one field");
            if let Some(var) = source.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
                sources.insert(resolve_env_var(var));
            } else if source.starts_with('/') {
                sources.insert(source.to_string());
            } else {
                assert!(
                    named_volumes.contains(source),
                    "service `{service_name}` mounts `{source}`, which is neither a host \
                     path, an env indirection, nor a declared named volume"
                );
            }
        }
    }
    assert!(
        !sources.is_empty(),
        "the compose file declares no bind mounts, so this fence would pass vacuously"
    );
    sources
}

/// The register a task loops over, for a `loop: "{{ <register>.results }}"`.
fn looped_register(task: &Mapping) -> Option<String> {
    task.get(Value::from("loop"))?
        .as_str()?
        .trim()
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map(str::trim)
        .and_then(|inner| inner.strip_suffix(".results"))
        .map(str::to_string)
}

/// Every concrete value a task's templated field takes across its loop. Three
/// shapes are modelled: no loop, a literal list, and a loop over a registered
/// stat's `results`. Anything else is a hard stop.
fn loop_expansion(tasks: &[Mapping], task: &Mapping, field: &[&str]) -> Vec<String> {
    let name = task_name(task);
    let expr = string_at(task, field)
        .unwrap_or_else(|| panic!("`{name}` must name its {}", field.join(".")));

    match task.get(Value::from("loop")) {
        None => vec![render_in_role(&expr, BTreeMap::new())],
        Some(Value::Sequence(entries)) => entries
            .iter()
            .map(|entry| {
                let entry = entry
                    .as_str()
                    .unwrap_or_else(|| panic!("`{name}` loops over a non-string entry"));
                let item = render_in_role(entry, BTreeMap::new());
                render_in_role(
                    &expr,
                    BTreeMap::from([("item".to_string(), minijinja::Value::from(item))]),
                )
            })
            .collect(),
        Some(Value::String(over)) => {
            let register = looped_register(task).unwrap_or_else(|| {
                panic!(
                    "`{name}` loops over `{over}`, a shape this test cannot resolve; \
                     teach it that shape before relying on this fence"
                )
            });
            let source = tasks
                .iter()
                .find(|candidate| {
                    string_at(candidate, &["register"]).as_deref() == Some(register.as_str())
                })
                .unwrap_or_else(|| {
                    panic!("`{name}` loops over `{register}`, which no task registers")
                });
            assert!(
                source.contains_key(Value::from("ansible.builtin.stat")),
                "`{name}` loops over `{register}`, which is registered by something other \
                 than a stat; teach this test that shape before relying on this fence"
            );
            loop_expansion(tasks, source, &["ansible.builtin.stat", "path"])
                .into_iter()
                .map(|path| {
                    render_in_role(
                        &expr,
                        BTreeMap::from([(
                            "item".to_string(),
                            minijinja::context! { item => path },
                        )]),
                    )
                })
                .collect()
        }
        Some(other) => panic!("`{name}` loops over an unmodelled shape: {other:?}"),
    }
}

/// A `file` task that creates directories, and the concrete paths it creates.
struct DirTask {
    index: usize,
    task: Mapping,
    paths: Vec<String>,
}

/// Every `ansible.builtin.file` task in the role, and the concrete paths it
/// names. `state: directory` is deliberately not required here: a task that only
/// *maintains* an existing path — `owner:` with no `state:` — is precisely the
/// shape that would reintroduce #630 while the build stayed green, so it has to
/// be in scope for the forbidden-field check even though it creates nothing.
fn file_tasks(tasks: &[Mapping]) -> Vec<DirTask> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.contains_key(Value::from("ansible.builtin.file")))
        .map(|(index, task)| DirTask {
            index,
            task: task.clone(),
            paths: loop_expansion(tasks, task, &["ansible.builtin.file", "path"]),
        })
        .collect()
}

/// Shapes this fence cannot read are hard stops, not silent passes. ansible-lint
/// rejects the short module form at the `production` profile, but a fence that
/// leans on another tool's configuration is a fence with a configurable hole —
/// and a shell that chowns is unreadable by construction.
fn assert_every_shape_is_readable(tasks: &[Mapping]) {
    for task in tasks {
        let name = task_name(task);
        for short in ["file", "command", "shell", "raw"] {
            assert!(
                !task.contains_key(Value::from(short)),
                "`{name}` uses the short `{short}:` form, which this fence does not read; \
                 name the module fully so the create-only rule can be checked"
            );
        }
        for module in ["ansible.builtin.command", "ansible.builtin.shell"] {
            let Some(argv) = task.get(Value::from(module)) else {
                continue;
            };
            let rendered = format!("{argv:?}");
            for verb in ["chown", "chmod"] {
                assert!(
                    !rendered.contains(verb),
                    "`{name}` runs `{verb}` through a shell, which this fence cannot read. \
                     Ownership and mode of a container-owned path are the image's business \
                     (#630, ADR-0036)"
                );
            }
        }
    }
}

fn created_directories(tasks: &[Mapping]) -> Vec<DirTask> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| {
            string_at(task, &["ansible.builtin.file", "state"]).as_deref() == Some("directory")
        })
        .map(|(index, task)| DirTask {
            index,
            task: task.clone(),
            paths: loop_expansion(tasks, task, &["ansible.builtin.file", "path"]),
        })
        .collect()
}

/// The role-created directories a container owns the interior of, keyed by path:
/// the intersection of what the role creates with what the compose file
/// bind-mounts. Neither side is selected by name, so the overlap is a real
/// finding rather than a restatement of how it was found.
fn container_owned(tasks: &[Mapping]) -> BTreeMap<String, DirTask> {
    let sources = compose_bind_sources();
    let mut owned = BTreeMap::new();
    for dir in created_directories(tasks) {
        for path in dir.paths.iter().filter(|path| sources.contains(*path)) {
            owned.insert(
                path.clone(),
                DirTask {
                    index: dir.index,
                    task: dir.task.clone(),
                    paths: dir.paths.clone(),
                },
            );
        }
    }
    owned
}

struct Guard {
    when: String,
    stat_index: usize,
    stat_register: String,
    stat_paths: Vec<String>,
}

fn when_expr(task: &Mapping) -> Option<String> {
    match task.get(Value::from("when"))? {
        Value::String(when) => Some(when.clone()),
        Value::Sequence(clauses) => Some(
            clauses
                .iter()
                .map(|clause| {
                    format!(
                        "({})",
                        clause.as_str().expect("a `when` clause must be a string")
                    )
                })
                .collect::<Vec<_>>()
                .join(" and "),
        ),
        other => panic!("an unmodelled `when` shape: {other:?}"),
    }
}

/// The stat task whose registered fact a create-only guard consults. The stat is
/// found through the `when` rather than by the path it names, so comparing its
/// paths against the created one is a real assertion.
fn guard_for(tasks: &[Mapping], path: &str, dir: &DirTask) -> Guard {
    let name = task_name(&dir.task);
    let when = when_expr(&dir.task).unwrap_or_else(|| {
        panic!(
            "`{name}` creates {path} unguarded, so every deploy re-applies it to a \
             directory the container owns (#630, ADR-0036)"
        )
    });

    // Two routes ground a create-only guard in a stat. A scalar task's `when`
    // names the registered fact; a looped task's `when` only ever sees `item`,
    // and the fact reaches it through `loop:` instead. Modelling one route
    // silently accepts no grounding at all for the other, so both are read here
    // and exactly one stat must answer.
    let looped = looped_register(&dir.task);
    let mut backing = tasks.iter().enumerate().filter(|(_, task)| {
        task.contains_key(Value::from("ansible.builtin.stat"))
            && string_at(task, &["register"]).is_some_and(|register| {
                when.contains(&register) || looped.as_deref() == Some(register.as_str())
            })
    });
    let (stat_index, stat) = backing
        .next()
        .unwrap_or_else(|| panic!("`when: {when}` on `{name}` consults no stat task"));
    assert!(
        backing.next().is_none(),
        "`when: {when}` on `{name}` consults more than one stat; the guard's grounding \
         is ambiguous"
    );

    Guard {
        when,
        stat_index,
        stat_register: string_at(stat, &["register"]).expect("checked above"),
        stat_paths: loop_expansion(tasks, stat, &["ansible.builtin.stat", "path"]),
    }
}

/// Evaluate the guard the way ansible would, for a directory that does or does
/// not already exist. Both the loop's `item` and the bare registered fact are
/// bound, so either guard shape renders — and strict mode fails a guard that
/// reaches for anything else.
fn guard_creates(guard: &Guard, exists: bool, path: &str) -> bool {
    let stat = minijinja::context! { exists => exists };
    let context = BTreeMap::from([
        (
            "item".to_string(),
            minijinja::context! { item => path, stat => stat.clone() },
        ),
        (
            guard.stat_register.clone(),
            minijinja::context! { stat => stat },
        ),
    ]);

    let rendered = strict_env()
        .render_str(
            &format!(
                "{{% if {} %}}create{{% else %}}skip{{% endif %}}",
                guard.when
            ),
            context,
        )
        .unwrap_or_else(|e| panic!("`when: {}` must evaluate: {e}", guard.when));
    rendered == "create"
}

#[test]
fn test_a_container_owned_directory_is_created_once_and_never_maintained() {
    let tasks = role_tasks();
    let owned = container_owned(&tasks);
    assert!(
        !owned.is_empty(),
        "no directory the role creates is bind-mounted into a container, so this fence \
         would pass vacuously"
    );

    assert_every_shape_is_readable(&tasks);

    // Every file task naming one of these paths is in scope, not only the task
    // that creates it. The omitted fields are what make a re-run harmless —
    // `file` with `state: directory` and no attributes does not touch an
    // existing directory — so a second task that merely maintains the same path
    // reintroduces the defect with the creator still clean.
    for task in file_tasks(&tasks) {
        let name = task_name(&task.task);
        for path in task.paths.iter().filter(|path| owned.contains_key(*path)) {
            for field in ["owner", "group", "mode"] {
                assert!(
                    string_at(&task.task, &["ansible.builtin.file", field]).is_none(),
                    "`{name}` declares `{field}` on {path}, whose interior a container \
                     owns. Ansible re-applies that on every deploy while the interior \
                     stays the container's uid, stripping that uid's search permission on \
                     the walk to its own files (#630, ADR-0036)"
                );
            }
            assert!(
                task.task.get(Value::from("notify")).is_none(),
                "`{name}` notifies a handler on {path}. Bouncing the stack so the \
                 entrypoint can undo an ownership reset ansible itself applied is not the \
                 fix; not applying it is (#630, ADR-0036)"
            );
        }
    }

    for (path, dir) in &owned {
        let name = task_name(&dir.task);
        let guard = guard_for(&tasks, path, dir);

        // Each grounding shape is asserted on its own terms. A scalar guard
        // names the register, so comparing the stat's path against the created
        // one is a real comparison. A looped guard derives the created path from
        // the stat's own loop, which makes that same comparison tautological —
        // what is worth asserting there is the structural coupling itself: the
        // path must dereference the loop item rather than name one of its own.
        let path_expr = string_at(&dir.task, &["ansible.builtin.file", "path"])
            .expect("a file task must name a path");
        if looped_register(&dir.task).is_some() {
            assert!(
                path_expr.contains("item.item"),
                "`{name}` loops over the stat's results but names `{path_expr}` rather \
                 than the loop's item, so its guard and the path it creates can disagree"
            );
        } else {
            assert!(
                guard.stat_paths.iter().any(|stated| stated == path),
                "`{name}` is guarded by a stat of {:?}, which never looks at {path}",
                guard.stat_paths
            );
        }
        assert!(
            guard.stat_index < dir.index,
            "the stat must run before the task whose guard reads it"
        );
        assert!(
            guard_creates(&guard, false, path),
            "`{name}` must create {path} when it is absent"
        );
        assert!(
            !guard_creates(&guard, true, path),
            "`{name}` must leave {path} alone once it exists"
        );
    }
}

#[test]
fn test_a_directory_no_container_owns_keeps_the_ownership_the_role_declares() {
    let tasks = role_tasks();
    let owned = container_owned(&tasks);
    let mut checked = 0;

    for dir in created_directories(&tasks) {
        let name = task_name(&dir.task);
        // The two regimes are opposites — one forbids these fields, the other
        // requires them — so a task straddling both cannot satisfy either. Say
        // that once, rather than leaving an author to reconcile two
        // contradictory failures from the same task.
        let (container, ansible): (Vec<_>, Vec<_>) =
            dir.paths.iter().partition(|path| owned.contains_key(*path));
        assert!(
            container.is_empty() || ansible.is_empty(),
            "`{name}` creates {container:?}, which a container owns, alongside \
             {ansible:?}, which ansible maintains. No single task can satisfy both \
             regimes; split it."
        );

        for path in ansible {
            for field in ["owner", "group", "mode"] {
                assert!(
                    string_at(&dir.task, &["ansible.builtin.file", field]).is_some(),
                    "`{name}` declares no `{field}` on {path}, which no container owns. \
                     Ansible is the only writer of that directory, so leaving its \
                     ownership and mode to the umask is a loss, not a create-only guard"
                );
            }
            checked += 1;
        }
    }

    assert!(
        checked > 0,
        "the role creates no ansible-owned directory, so this fence would pass vacuously"
    );
}

#[test]
fn test_every_compose_bind_source_resolves_to_an_absolute_host_path() {
    for source in compose_bind_sources() {
        assert!(
            source.starts_with('/'),
            "`{source}` is not an absolute host path; the compose file's indirection \
             resolved to something this fence cannot compare against a created directory"
        );
    }
}
