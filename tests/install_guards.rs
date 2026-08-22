use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Sequence, Value};

/// A role that records what it installed in a sidecar `version` file it writes
/// itself. The marker is a note about the past, so on its own it cannot answer
/// whether the artifact is still there -- deleting the artifact to force a
/// re-install leaves the marker intact and the role reports converged while
/// systemd has nothing to exec (#591, #596). Every role here grounds its
/// installed-version fact in a stat of the artifact, so a missing artifact
/// reads as "nothing installed" and every guard downstream inherits that.
///
/// Grimmory left this regime in #597: its jar filename carries the version, so
/// the dest cannot already exist at a new version and the guard is a bare
/// existence test with no marker to reconcile. That is the better shape, and it
/// needs a single-file download to work -- these five unpack an archive into a
/// fixed layout, where versioning the dest means versioning the directory.
struct MarkerRole {
    role: &'static str,
    /// The defaults key the stat, the install and the unit all resolve through.
    artifact_var: &'static str,
    /// What that key must expand to.
    artifact_value: &'static str,
    /// The path the stat watches. Either the artifact itself or a file inside
    /// it that the unit depends on.
    sentinel: &'static str,
    /// Unit templates that must name the artifact.
    units: &'static [&'static str],
    /// The directive in those units that names it.
    unit_directive: &'static str,
}

const MARKER_ROLES: &[MarkerRole] = &[
    MarkerRole {
        role: "bichon",
        artifact_var: "bichon_binary_path",
        artifact_value: "{{ bichon_install_dir }}/bichon-server",
        sentinel: "{{ bichon_binary_path }}",
        units: &["bichon.service.j2"],
        unit_directive: "ExecStart=",
    },
    MarkerRole {
        role: "colporteur",
        artifact_var: "colporteur_binary_path",
        artifact_value: "{{ colporteur_install_path }}/colporteur",
        sentinel: "{{ colporteur_binary_path }}",
        units: &["colporteur.service.j2"],
        unit_directive: "ExecStart=",
    },
    MarkerRole {
        role: "gokapi",
        artifact_var: "gokapi_binary_path",
        artifact_value: "{{ gokapi_install_path }}/gokapi",
        sentinel: "{{ gokapi_binary_path }}",
        units: &["gokapi.service.j2"],
        unit_directive: "ExecStart=",
    },
    MarkerRole {
        role: "headscale",
        artifact_var: "headscale_binary_path",
        artifact_value: "/usr/local/bin/headscale",
        sentinel: "{{ headscale_binary_path }}",
        units: &["headscale.service.j2"],
        unit_directive: "ExecStart=",
    },
    // The tarball lands a source tree and the venv is built separately, so
    // "installed" is not one file. All four units run out of the source tree
    // and the consumer execs `manage.py` from it, so that is the file whose
    // absence means the tree has to come back.
    MarkerRole {
        role: "paperless",
        artifact_var: "paperless_src_dir",
        artifact_value: "{{ paperless_install_path }}/src",
        sentinel: "{{ paperless_src_dir }}/manage.py",
        units: &[
            "paperless-consumer.service.j2",
            "paperless-scheduler.service.j2",
            "paperless-task-queue.service.j2",
            "paperless-webserver.service.j2",
        ],
        unit_directive: "WorkingDirectory=",
    },
];

fn role_dir(role: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ansible/roles")
        .join(role)
}

/// One task with the guards it actually runs under: a `when` on an enclosing
/// block ANDs into every task inside it, which is how `bichon` and `paperless`
/// gate their installs.
struct Task {
    body: Mapping,
    guards: Vec<String>,
}

fn whens(task: &Mapping) -> Vec<String> {
    match task.get(Value::from("when")) {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Sequence(all)) => all
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn flatten(tasks: &Sequence, inherited: &[String], out: &mut Vec<Task>) {
    for task in tasks {
        let Some(body) = task.as_mapping() else {
            continue;
        };
        let mut scoped = inherited.to_vec();
        scoped.extend(whens(body));
        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = body.get(Value::from(section)).and_then(Value::as_sequence) {
                flatten(inner, &scoped, out);
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

fn role_tasks(role: &str) -> Vec<Task> {
    let raw = fs::read_to_string(role_dir(role).join("tasks/main.yml"))
        .unwrap_or_else(|_| panic!("{role} tasks"));
    let parsed: Sequence =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{role} tasks must parse: {e}"));
    let mut tasks = Vec::new();
    flatten(&parsed, &[], &mut tasks);
    tasks
}

fn string_at(task: &Mapping, path: &[&str]) -> Option<String> {
    let mut node = &Value::Mapping(task.clone());
    for key in path {
        node = node.get(*key)?;
    }
    node.as_str().map(str::to_string)
}

fn register(task: &Mapping) -> Option<String> {
    string_at(task, &["register"])
}

/// Where the role's only task invoking `module` sits in play order.
fn sole_task_using(tasks: &[Task], role: &str, module: &str) -> usize {
    let mut found = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.body.contains_key(Value::from(module)));
    let (index, _) = found
        .next()
        .unwrap_or_else(|| panic!("the {role} role must have a {module} task"));
    assert!(
        found.next().is_none(),
        "{module} is no longer unique in the {role} role; this test can no longer identify the install"
    );
    index
}

/// The decision to install, taken apart: the fact that says what is installed,
/// the two stats it consults, and the download it gates. Nothing here is found
/// by the path it names, so comparing those paths is a real assertion rather
/// than a restatement of how they were found.
struct InstallGuard {
    fact: String,
    slurp_register: String,
    marker_path: String,
    marker_stat_register: String,
    artifact_stat_register: String,
    artifact_stat_path: String,
    download_guards: Vec<String>,
}

fn install_guard(spec: &MarkerRole) -> InstallGuard {
    let role = spec.role;
    let tasks = role_tasks(role);
    let fact_key = format!("{role}_installed_version");

    let mut facts = tasks.iter().enumerate().filter(|(_, task)| {
        string_at(&task.body, &["ansible.builtin.set_fact", &fact_key]).is_some()
    });
    let (fact_index, fact_task) = facts
        .next()
        .unwrap_or_else(|| panic!("the {role} role must set {fact_key}"));
    assert!(
        facts.next().is_none(),
        "{fact_key} is set more than once in {role}; which one guards the install is ambiguous"
    );
    let fact = string_at(&fact_task.body, &["ansible.builtin.set_fact", &fact_key])
        .expect("checked above");

    let consulted = |task: &Task| register(&task.body).is_some_and(|reg| fact.contains(&reg));
    let before_fact = &tasks[..fact_index];

    let mut slurps = before_fact.iter().filter(|task| {
        task.body.contains_key(Value::from("ansible.builtin.slurp")) && consulted(task)
    });
    let slurp = slurps
        .next()
        .unwrap_or_else(|| panic!("{fact_key} must read a version marker"));
    assert!(
        slurps.next().is_none(),
        "{fact_key} reads more than one file; its provenance is ambiguous"
    );
    let marker_path = string_at(&slurp.body, &["ansible.builtin.slurp", "src"])
        .expect("the slurp must name a src");

    let stats: Vec<&Task> = before_fact
        .iter()
        .filter(|task| {
            task.body.contains_key(Value::from("ansible.builtin.stat")) && consulted(task)
        })
        .collect();
    let stat_path = |task: &Task| {
        string_at(&task.body, &["ansible.builtin.stat", "path"]).expect("a stat must name a path")
    };

    let mut artifact_stats = stats.iter().filter(|task| stat_path(task) != marker_path);
    let artifact_stat = artifact_stats.next().unwrap_or_else(|| {
        panic!("{fact_key} consults nothing but {marker_path}, a note the role wrote itself")
    });
    assert!(
        artifact_stats.next().is_none(),
        "{fact_key} consults two stats besides {marker_path}; which grounds it is ambiguous"
    );

    let mut marker_stats = stats.iter().filter(|task| stat_path(task) == marker_path);
    let marker_stat = marker_stats
        .next()
        .unwrap_or_else(|| panic!("{fact_key} must guard the slurp of {marker_path} with a stat"));

    let download_index = sole_task_using(&tasks, role, "ansible.builtin.get_url");
    assert!(
        fact_index < download_index,
        "{fact_key} must be set before the download it gates"
    );

    InstallGuard {
        fact,
        slurp_register: register(&slurp.body).expect("checked above"),
        marker_path,
        marker_stat_register: register(&marker_stat.body).expect("checked above"),
        artifact_stat_register: register(&artifact_stat.body).expect("checked above"),
        artifact_stat_path: stat_path(artifact_stat),
        download_guards: tasks[download_index].guards.clone(),
    }
}

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64encode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut block = [0u8; 3];
        block[..chunk.len()].copy_from_slice(chunk);
        let packed = u32::from_be_bytes([0, block[0], block[1], block[2]]);
        for slot in 0..4 {
            if slot <= chunk.len() {
                out.push(B64[(packed >> (18 - slot * 6)) as usize & 0x3f] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Ansible decodes a slurp's payload with this filter; minijinja has no
/// equivalent, so the guard's own expression cannot be evaluated without it.
/// Really decoding, rather than an identity filter over plaintext, is what makes
/// a fact that drops `| b64decode` fail: slurp hands over base64, and an
/// undecoded marker matches no pinned version.
fn b64decode(encoded: String) -> Result<String, minijinja::Error> {
    let mut bits = 0u32;
    let mut width = 0u32;
    let mut out = Vec::new();
    for byte in encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && *byte != b'=')
    {
        let index = B64
            .iter()
            .position(|candidate| *candidate == byte)
            .ok_or_else(|| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, "not base64")
            })?;
        bits = (bits << 6) | index as u32;
        width += 6;
        if width >= 8 {
            width -= 8;
            out.push((bits >> width) as u8);
        }
    }
    String::from_utf8(out)
        .map_err(|_| minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, "not utf-8"))
}

fn environment() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.add_filter("b64decode", b64decode);
    env
}

/// What the role concludes is installed, given what is on disk. The marker is
/// only ever consulted through this, so this is where the artifact has to enter.
fn installed_version(
    guard: &InstallGuard,
    marker_says: &str,
    marker_exists: bool,
    artifact_exists: bool,
) -> String {
    let stat = |exists: bool| BTreeMap::from([("stat", BTreeMap::from([("exists", exists)]))]);
    let context = BTreeMap::from([
        (
            guard.slurp_register.clone(),
            minijinja::Value::from_serialize(BTreeMap::from([("content", b64encode(marker_says))])),
        ),
        (
            guard.marker_stat_register.clone(),
            minijinja::Value::from_serialize(stat(marker_exists)),
        ),
        (
            guard.artifact_stat_register.clone(),
            minijinja::Value::from_serialize(stat(artifact_exists)),
        ),
    ]);
    environment()
        .render_str(&guard.fact, context)
        .unwrap_or_else(|e| {
            panic!(
                "the installed-version fact must evaluate: {e}\n{}",
                guard.fact
            )
        })
}

/// Whether the install runs, evaluating every guard it inherits the way ansible
/// would -- a `when` on an enclosing block ANDs into the task's own.
fn installs(spec: &MarkerRole, guard: &InstallGuard, installed: &str, pinned: &str) -> bool {
    let fact_key = format!("{}_installed_version", spec.role);
    let context = BTreeMap::from([
        (fact_key, minijinja::Value::from(installed)),
        (
            format!("{}_version", spec.role),
            minijinja::Value::from(pinned),
        ),
    ]);
    let conjunction = guard
        .download_guards
        .iter()
        .map(|clause| format!("({clause})"))
        .collect::<Vec<_>>()
        .join(" and ");
    assert!(
        !conjunction.is_empty(),
        "{}'s download is unguarded; it would re-download on every deploy",
        spec.role
    );
    let rendered = environment()
        .render_str(
            &format!("{{% if {conjunction} %}}install{{% else %}}skip{{% endif %}}"),
            context,
        )
        .unwrap_or_else(|e| panic!("`when: {conjunction}` must evaluate: {e}"));
    rendered == "install"
}

/// The scenario the marker cannot see: the artifact is deleted to force a
/// re-install -- the recovery path for a bad release asset, exercised for #586
/// -- and the marker still names the pinned version.
#[test]
fn test_a_missing_artifact_reinstalls_even_when_the_marker_matches() {
    for spec in MARKER_ROLES {
        let guard = install_guard(spec);
        let installed = installed_version(&guard, "9.9.9", true, false);
        assert_eq!(
            installed, "",
            "{}: an absent artifact must read as nothing installed, not as {} says",
            spec.role, guard.marker_path
        );
        assert!(
            installs(spec, &guard, &installed, "9.9.9"),
            "{}: deleting {} must bring it back; a version marker must not veto that",
            spec.role,
            spec.sentinel
        );
    }
}

#[test]
fn test_a_converged_install_is_left_alone() {
    for spec in MARKER_ROLES {
        let guard = install_guard(spec);
        let installed = installed_version(&guard, "9.9.9", true, true);
        assert_eq!(
            installed, "9.9.9",
            "{}: marker must be read verbatim",
            spec.role
        );
        assert!(
            !installs(spec, &guard, &installed, "9.9.9"),
            "{}: a converged install must stay idempotent",
            spec.role
        );
    }
}

#[test]
fn test_a_version_bump_reinstalls_a_present_artifact() {
    for spec in MARKER_ROLES {
        let guard = install_guard(spec);
        let installed = installed_version(&guard, "9.9.9", true, true);
        assert!(
            installs(spec, &guard, &installed, "10.0.0"),
            "{}: a bump past what the marker names must install",
            spec.role
        );
    }
}

#[test]
fn test_a_fresh_host_installs() {
    for spec in MARKER_ROLES {
        let guard = install_guard(spec);
        let installed = installed_version(&guard, "", false, false);
        assert_eq!(
            installed, "",
            "{}: nothing on disk is nothing installed",
            spec.role
        );
        assert!(
            installs(spec, &guard, &installed, "9.9.9"),
            "{}: a host with neither marker nor artifact must install",
            spec.role
        );
    }
}

/// An artifact with no marker beside it -- a restore that dropped the sidecar,
/// or an install predating the marker -- is reinstalled rather than assumed
/// current. The reverse of the bug: reality is unreadable, so nothing is claimed.
#[test]
fn test_an_unmarked_artifact_is_reinstalled() {
    for spec in MARKER_ROLES {
        let guard = install_guard(spec);
        let installed = installed_version(&guard, "", false, true);
        assert_eq!(
            installed, "",
            "{}: an artifact with no marker beside it names no version",
            spec.role
        );
        assert!(
            installs(spec, &guard, &installed, "9.9.9"),
            "{}: an unreadable install must be redone, not assumed current",
            spec.role
        );
    }
}

#[test]
fn test_the_stat_watches_the_artifact_and_not_the_marker() {
    for spec in MARKER_ROLES {
        let guard = install_guard(spec);
        assert_eq!(
            guard.artifact_stat_path, spec.sentinel,
            "{}: the guard must be grounded in the artifact it protects",
            spec.role
        );
        assert_ne!(
            guard.artifact_stat_path, guard.marker_path,
            "{}: statting the marker proves only that the role wrote it",
            spec.role
        );
    }
}

#[test]
fn test_the_units_run_the_artifact_the_guard_watches() {
    for spec in MARKER_ROLES {
        let reference = format!("{{{{ {} }}}}", spec.artifact_var);
        assert!(
            spec.sentinel.starts_with(&reference),
            "{}: the sentinel must resolve through {}",
            spec.role,
            spec.artifact_var
        );
        for unit in spec.units {
            let template = fs::read_to_string(role_dir(spec.role).join("templates").join(unit))
                .unwrap_or_else(|_| panic!("{unit} must exist"));
            let directive = template
                .lines()
                .find(|line| line.starts_with(spec.unit_directive))
                .unwrap_or_else(|| panic!("{unit} must have a {}", spec.unit_directive));
            assert!(
                directive.contains(&reference),
                "{unit} runs a different artifact than the guard watches:\n{directive}"
            );
        }
    }
}

#[test]
fn test_the_artifact_path_has_one_definition() {
    for spec in MARKER_ROLES {
        let defaults = fs::read_to_string(role_dir(spec.role).join("defaults/main.yml"))
            .unwrap_or_else(|_| panic!("{} defaults", spec.role));
        let parsed: Value = serde_yaml::from_str(&defaults)
            .unwrap_or_else(|e| panic!("{} defaults must parse: {e}", spec.role));
        assert_eq!(
            parsed[spec.artifact_var].as_str(),
            Some(spec.artifact_value),
            "the stat, the install and the unit all resolve {} through this default",
            spec.artifact_var
        );
    }
}

fn all_roles() -> Vec<String> {
    let mut roles: Vec<String> =
        fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles"))
            .expect("ansible/roles must exist")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
    roles.sort();
    roles
}

/// Every task in the role, across all of its task files. Order is meaningless
/// here, so this is for membership questions only.
fn every_task(role: &str) -> Vec<Task> {
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
    let mut tasks = Vec::new();
    for file in files {
        let raw = fs::read_to_string(&file).expect("a listed task file must be readable");
        let parsed: Sequence = serde_yaml::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} must parse: {e}", file.display()));
        flatten(&parsed, &[], &mut tasks);
    }
    tasks
}

/// Paths whose contents the role authors: a `copy` rendering inline `content:`,
/// or a `template`. A `copy` from a `src:` carries bytes from somewhere else, so
/// its destination is an artifact, not a note -- which is what separates
/// headscale's binary (copied from /tmp) from its version marker.
fn authored_paths(tasks: &[Task]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for task in tasks {
        let copy_note = task
            .body
            .get(Value::from("ansible.builtin.copy"))
            .filter(|copy| copy.get("content").is_some())
            .and_then(|copy| copy.get("dest"));
        let templated = task
            .body
            .get(Value::from("ansible.builtin.template"))
            .and_then(|template| template.get("dest"));
        for dest in [copy_note, templated].into_iter().flatten() {
            if let Some(dest) = dest.as_str() {
                paths.insert(dest.to_string());
            }
        }
    }
    paths
}

/// A role reading an installed version out of a file it wrote itself, and
/// whether that reading also consults something the role did not write.
struct MarkerFact {
    role: String,
    key: String,
    marker: String,
    grounded_in: Option<String>,
}

fn marker_facts() -> Vec<MarkerFact> {
    let mut facts = Vec::new();
    for role in all_roles() {
        let tasks = every_task(&role);
        let authored = authored_paths(&tasks);

        let notes: BTreeMap<String, String> = tasks
            .iter()
            .filter_map(|task| {
                let src = string_at(&task.body, &["ansible.builtin.slurp", "src"])?;
                let register = register(&task.body)?;
                authored.contains(&src).then_some((register, src))
            })
            .collect();
        if notes.is_empty() {
            continue;
        }

        let unauthored_stats: Vec<(String, String)> = tasks
            .iter()
            .filter_map(|task| {
                let path = string_at(&task.body, &["ansible.builtin.stat", "path"])?;
                let register = register(&task.body)?;
                (!authored.contains(&path)).then_some((register, path))
            })
            .collect();

        for task in &tasks {
            let Some(Value::Mapping(assignments)) =
                task.body.get(Value::from("ansible.builtin.set_fact"))
            else {
                continue;
            };
            for (key, expression) in assignments {
                let (Some(key), Some(expression)) = (key.as_str(), expression.as_str()) else {
                    continue;
                };
                if !key.ends_with("_installed_version") {
                    continue;
                }
                let Some(marker) = notes
                    .iter()
                    .find(|(register, _)| expression.contains(*register))
                    .map(|(_, src)| src.clone())
                else {
                    continue;
                };
                facts.push(MarkerFact {
                    role: role.clone(),
                    key: key.to_string(),
                    marker,
                    grounded_in: unauthored_stats
                        .iter()
                        .find(|(register, _)| expression.contains(register))
                        .map(|(_, path)| path.clone()),
                });
            }
        }
    }
    facts
}

/// The fence. A role that reads its installed version out of a file it wrote
/// itself has recorded intent, not reality; it must also consult something it
/// did not write. Fails the build when a new role reintroduces the marker-only
/// guard, so this cannot be filed a fourth time.
#[test]
fn test_no_installed_version_trusts_only_a_note_the_role_wrote() {
    let ungrounded: Vec<String> = marker_facts()
        .iter()
        .filter(|fact| fact.grounded_in.is_none())
        .map(|fact| format!("{}: {} trusts only {}", fact.role, fact.key, fact.marker))
        .collect();
    assert!(
        ungrounded.is_empty(),
        "a version marker records what the role once installed, not what is installed now. \
         Stat the artifact and fold it into the fact:\n  {}",
        ungrounded.join("\n  ")
    );
}

/// Keeps MARKER_ROLES honest. A new role on the marker regime has to be
/// declared here, which is what subjects it to the assertions above -- the
/// sentinel, the unit grounding, the single defaults definition and the four
/// install scenarios.
#[test]
fn test_every_marker_role_is_covered_by_this_test() {
    let detected: BTreeSet<String> = marker_facts()
        .iter()
        .map(|fact| fact.role.clone())
        .collect();
    let declared: BTreeSet<String> = MARKER_ROLES
        .iter()
        .map(|spec| spec.role.to_string())
        .collect();
    assert_eq!(
        detected, declared,
        "roles reading an installed version from their own marker must be declared in MARKER_ROLES"
    );
}
