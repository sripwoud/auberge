use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Sequence, Value};

fn roles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles")
}

/// Handlers whose flush must stay at end of play.
///
/// `Restart caddy` is checked by the `ingress_gate` post_task instead (#568),
/// against the whole assembled config: a vhost binding an address the host does
/// not own takes every other site on the box down, so that restart is judged
/// once per play rather than per role.
const DEFERRED_HANDLERS: &[&str] = &["Restart caddy"];

/// `(role, handler)` pairs where the role applies the handler's effect itself,
/// with an explicit task ahead of the probe, so the probe does not depend on the
/// queued restart to be reading current config.
///
/// ssh notifies `Restart sshd` and then reloads sshd outright in
/// `tasks/validate.yml` before probing the new port. Flushing instead would
/// restart sshd mid-port-change — the lockout the whole validate block exists to
/// avoid — and a reload has already proven the config loads.
const SELF_APPLIED: &[(&str, &str)] = &[("ssh", "Restart sshd")];

fn field<'a>(task: &'a Mapping, key: &str) -> Option<&'a Value> {
    task.get(Value::String(key.to_string()))
}

/// The file an `include_tasks` names. A non-string value is a shape this model
/// cannot resolve, and silently skipping it would hide whatever the file probes.
fn included_file(task: &Mapping) -> Option<String> {
    let include = field(task, "ansible.builtin.include_tasks")?;
    Some(
        include
            .as_str()
            .unwrap_or_else(|| panic!("include_tasks: {include:?} is not a bare file name"))
            .to_string(),
    )
}

fn parse_tasks(path: &Path) -> Sequence {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
    serde_yaml::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} must parse as a task list: {e}", path.display()))
}

/// Tasks in the order the play runs them, with `block`/`rescue`/`always` and
/// included task files flattened in place.
///
/// A `when` on an enclosing block ANDs into every task inside it, which this
/// model does not represent — the ordering asserted here is independent of
/// guards, so a conditional block flattens like any other. Includes are resolved
/// because ssh notifies `Restart sshd` in `main.yml` and probes the new port from
/// `validate.yml`: stopping at `main.yml` would leave that pair unseen, and it is
/// the one pair this file has to grant an explicit exception.
fn flatten(tasks: &Sequence, dir: &Path, out: &mut Vec<Mapping>) {
    for task in tasks {
        let Some(task) = task.as_mapping() else {
            continue;
        };
        if let Some(file) = included_file(task) {
            flatten(&parse_tasks(&dir.join(file)), dir, out);
            continue;
        }
        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = field(task, section).and_then(Value::as_sequence) {
                flatten(inner, dir, out);
                nested = true;
            }
        }
        if !nested {
            out.push(task.clone());
        }
    }
}

fn role_tasks(role: &str) -> Vec<Mapping> {
    let dir = roles_dir().join(role).join("tasks");
    let entry = dir.join("main.yml");
    if !entry.exists() {
        return Vec::new();
    }
    let mut tasks = Vec::new();
    flatten(&parse_tasks(&entry), &dir, &mut tasks);
    tasks
}

fn roles() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(roles_dir())
        .expect("ansible/roles must exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

fn task_name(task: &Mapping) -> String {
    field(task, "name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_string()
}

/// Handlers a task notifies, minus the ones whose flush is deliberately deferred.
fn notified_handlers(task: &Mapping) -> Vec<String> {
    let notified = match field(task, "notify") {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Sequence(many)) => many
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };
    notified
        .into_iter()
        .filter(|handler| !DEFERRED_HANDLERS.contains(&handler.as_str()))
        .collect()
}

/// A task reads the service the role is deploying, so its result is a verdict on
/// whichever binary and config that service is running right now.
///
/// What matters is where the call lands, not whether it is delegated: ssh probes
/// the target's own sshd *from* the controller, so `delegate_to` says nothing.
/// `wait_for` always lands on the host being converged — a path on its disk, or a
/// port on an address it owns. `uri` reaches wherever its URL points, and only a
/// loopback URL is this host: blocky and colporteur fetch release checksums,
/// blocky drives the Tailscale API, dns_record drives Cloudflare's.
///
/// `wait_for_connection` is absent on purpose. It probes SSH reachability, not a
/// managed service, and cockpit uses it to survive a `netplan apply`.
fn is_probe(task: &Mapping) -> bool {
    if field(task, "ansible.builtin.wait_for").is_some() {
        return true;
    }
    let Some(url) = field(task, "ansible.builtin.uri")
        .and_then(Value::as_mapping)
        .and_then(|args| field(args, "url"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    url.contains("127.0.0.1") || url.contains("localhost")
}

fn is_flush(task: &Mapping) -> bool {
    field(task, "ansible.builtin.meta").and_then(Value::as_str) == Some("flush_handlers")
}

struct UnflushedNotify {
    role: String,
    handler: String,
    notifier: String,
    probe: String,
}

/// Every notify that reaches a probe with no flush in between, so the probe
/// reports on the process the notify was queued to replace. Reported once per
/// notify, naming the earliest probe it reaches.
///
/// Every probe is considered, not just the role's first: a role that probes twice
/// can queue a restart after the first probe and still read a stale process at
/// the second.
fn unflushed_notifies() -> Vec<UnflushedNotify> {
    let mut findings = Vec::new();

    for role in roles() {
        let tasks = role_tasks(&role);
        let probes: Vec<usize> = (0..tasks.len()).filter(|i| is_probe(&tasks[*i])).collect();
        let flushes: Vec<usize> = (0..tasks.len()).filter(|i| is_flush(&tasks[*i])).collect();

        for (index, task) in tasks.iter().enumerate() {
            for handler in notified_handlers(task) {
                if SELF_APPLIED.contains(&(role.as_str(), handler.as_str())) {
                    continue;
                }
                let Some(probe) = probes
                    .iter()
                    .filter(|probe| **probe > index)
                    .find(|probe| !flushes.iter().any(|flush| (index..**probe).contains(flush)))
                else {
                    continue;
                };
                findings.push(UnflushedNotify {
                    role: role.clone(),
                    handler,
                    notifier: task_name(task),
                    probe: task_name(&tasks[*probe]),
                });
            }
        }
    }

    findings
}

/// The probe each role reads its own service through, so a change to `is_probe`
/// that stops seeing one shows up here rather than as a quietly passing suite.
fn probes_by_role() -> BTreeMap<String, Vec<String>> {
    let mut found = BTreeMap::new();
    for role in roles() {
        let names: Vec<String> = role_tasks(&role)
            .iter()
            .filter(|task| is_probe(task))
            .map(task_name)
            .collect();
        if !names.is_empty() {
            found.insert(role, names);
        }
    }
    found
}

#[test]
fn test_the_scan_sees_the_probe_in_every_role_that_has_one() {
    let probing = probes_by_role();
    for role in ["grimmory", "baikal", "blocky", "ssh", "syncthing"] {
        assert!(
            probing.contains_key(role),
            "{role} reads its own service and must be scanned; found {probing:?}"
        );
    }
}

#[test]
fn test_an_outbound_api_call_is_not_a_probe() {
    let probing = probes_by_role();
    assert!(
        !probing.contains_key("dns_record"),
        "dns_record only ever calls Cloudflare, deploys no service of its own, and \
         must not stand in for a readiness probe; found {probing:?}"
    );
}

#[test]
fn test_a_pending_restart_lands_before_the_probe_that_validates_it() {
    let findings = unflushed_notifies();
    assert!(
        findings.is_empty(),
        "handlers flush at end of play, so these probes report on the process still \
         running the pre-deploy binary and config, and the replacement goes live \
         unvalidated (#594). Add `ansible.builtin.meta: flush_handlers` before the probe:\n{}",
        findings
            .iter()
            .map(|f| format!(
                "  {}: `{}` notifies `{}`, then `{}` probes with no flush between",
                f.role, f.notifier, f.handler, f.probe
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
