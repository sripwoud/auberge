//! Two fences over the Readiness Probe, one recognizer ([`is_probe`]):
//!
//! - **Ordering**: every notify a role queues is flushed before the role's
//!   next probe, so the probe reads the process the deploy installed rather
//!   than the one it replaced (#594).
//! - **Presence** (#720): every App whose Unit Ownership includes a Serving
//!   Unit carries at least one probe across its roles. Ordering alone is
//!   conditional on presence — an App with zero probes contributes zero
//!   (notify, probe) pairs and passes vacuously, which is how headscale
//!   deployed unread until #716.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_yaml::{Mapping, Sequence, Value};

mod common;

use common::apps::{app_of, declared_units};
use common::units::{Scope, fleet_units};
use common::{all_roles, field, role_dir, task_name};

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

/// Apps whose Serving Unit is read back by something other than a role-level
/// probe, permanently.
///
/// caddy: the Ingress Gate judges its restart once per play against the whole
/// assembled config (#568) — that is its probe by design, and the reason
/// `Restart caddy` sits in [`DEFERRED_HANDLERS`] above.
const PROBED_BY_THE_INGRESS_GATE: &[&str] = &["caddy"];

/// Apps that deploy a Serving Unit today and probe none of them — the #716
/// hole, held open per App and ratcheted rather than closed at once: an entry
/// leaves only by gaining a probe, and a new Serving App must probe or
/// visibly join this list in review. Each entry names what its future probe
/// would read, so the list doubles as the backlog's decomposition; the
/// per-App issue is filed when someone picks an entry up, not upfront.
const NOT_YET_PROBED: &[(&str, &str)] = &[
    (
        "actual",
        "wait_for TCP `actual_port` (5006), actual-server's loopback listener",
    ),
    (
        "bichon",
        "wait_for TCP `bichon_port` (15630), the daemon's loopback listener",
    ),
    (
        "calibre",
        "wait_for TCP `calibre_port` (8083), calibre-web's loopback listener",
    ),
    (
        "freshrss",
        "wait_for TCP `freshrss_port` (8084), the `php -S` loopback listener",
    ),
    (
        "gokapi",
        "wait_for TCP `gokapi_port` (53842), the binary's loopback listener",
    ),
    (
        "hermes",
        "nothing the repo names yet: `hermes gateway` long-polls Telegram, so \
         the probe target — a socket or state file under `hermes_config_dir` — \
         is the first thing the entry's work discovers",
    ),
    (
        "immich",
        "uri on loopback `immich_port` (2283) once `docker compose up --wait` \
         returns, e.g. /api/server/ping",
    ),
    (
        "navidrome",
        "wait_for TCP `navidrome_port` (4533), the deb-packaged server's \
         loopback listener",
    ),
    (
        "paperless",
        "wait_for TCP `paperless_port` (8000), granian's loopback listener",
    ),
    (
        "radio",
        "uri on loopback `radio_icecast_port` (8005), icecast's status page, \
         which liquidsoap's mount feeds",
    ),
    (
        "tgtg",
        "nothing listens: the bot long-polls Telegram, so a probe must \
         wait_for what it writes under `tgtg_data_dir`, the way syncthing's \
         probe reads its generated config.xml",
    ),
    (
        "vibecoder",
        "the port `start-telegram-webhook.js` binds for Telegram callbacks — \
         not stated in this repo; discovering it is the entry's first step",
    ),
];

/// A declared `.service` the scan holds no unit file for — known only through
/// drop-ins, or through no file at all — classified Serving by hand, the
/// DECLARED_WITHOUT_FILE treatment (tests/unit_ownership.rs): each entry is
/// checked to stay underivable, so the day a role starts templating the unit
/// file, its own `[Install]` section takes over and the entry must go.
const SERVING_WITHOUT_FILE: &[(&str, &str, &str)] = &[
    (
        "navidrome",
        "navidrome.service",
        "the deb's packaged unit, WantedBy=multi-user.target upstream; the \
         role only drops in over it",
    ),
    (
        "radio",
        "icecast2.service",
        "apt's packaged unit, enabled at install; the role only drops a \
         memory budget over it",
    ),
    (
        "syncthing",
        "syncthing@{admin_user}.service",
        "a packaged template unit the role only enables per user; there is \
         no file to read an [Install] section from",
    ),
];

/// The install targets a `.service` in the fleet hooks under: a boot target —
/// `multi-user.target` for the system manager, `default.target` for a user's
/// — marks a Serving Unit, and `timers.target` never appears on a service at
/// all (it marks the `.timer` that pulls one). Split so a target outside both
/// lists is a hard stop rather than a quiet "not Serving".
const BOOT_TARGETS: &[&str] = &["default.target", "multi-user.target"];
const NON_BOOT_TARGETS: &[&str] = &["timers.target"];

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
    let dir = role_dir(role).join("tasks");
    let entry = dir.join("main.yml");
    if !entry.exists() {
        return Vec::new();
    }
    let mut tasks = Vec::new();
    flatten(&parse_tasks(&entry), &dir, &mut tasks);
    tasks
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

    for role in all_roles() {
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
                    notifier: task_name(task).to_string(),
                    probe: task_name(&tasks[*probe]).to_string(),
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
    for role in all_roles() {
        let names: Vec<String> = role_tasks(&role)
            .iter()
            .filter(|task| is_probe(task))
            .map(|task| task_name(task).to_string())
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
    for role in [
        "grimmory",
        "baikal",
        "blocky",
        "headscale",
        "ssh",
        "syncthing",
    ] {
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

/// The Apps whose Unit Ownership (`units:` in the Playbook Meta, ADR-0042)
/// includes at least one Serving Unit: a `.service` installed `WantedBy` a
/// boot target, rather than pulled by a timer.
///
/// The Meta is the authority, not the file scan: Unit Ownership is already
/// fenced against the scan in both directions (tests/unit_ownership.rs) and
/// is the only inventory that holds the units no task reveals — a
/// scan-computed domain would reopen the #716 class for the next
/// packaged-template-unit role. The `[Install]` section of the unit's own
/// scanned file does the classifying; a unit whose file the scan does not
/// hold is hand-classified in [`SERVING_WITHOUT_FILE`], and one that is in
/// neither is a hard stop, because a unit this fence cannot place is a unit
/// it would pass over.
fn serving_apps() -> BTreeSet<String> {
    let mut install_targets: BTreeMap<(String, Scope), Vec<String>> = BTreeMap::new();
    for unit in fleet_units() {
        // A drop-in refines a unit another file (or a package) installs; only
        // the unit's own file carries the [Install] section `enable` reads.
        if unit.dropin.is_some() {
            continue;
        }
        let targets = unit
            .all_in("Install", "WantedBy")
            .iter()
            .flat_map(|value| value.split_whitespace())
            .map(str::to_string)
            .collect();
        install_targets.insert((unit.name.clone(), unit.scope), targets);
    }

    let mut out = BTreeSet::new();
    for owned in declared_units() {
        if !owned.unit.ends_with(".service") {
            continue;
        }
        let serving = match install_targets.get(&(owned.unit.clone(), owned.scope)) {
            Some(targets) => {
                for target in targets {
                    assert!(
                        BOOT_TARGETS.contains(&target.as_str())
                            || NON_BOOT_TARGETS.contains(&target.as_str()),
                        "{}: `WantedBy={target}` is a target this classifier does \
                         not know; decide whether it is a boot target before \
                         trusting the verdict",
                        owned.id()
                    );
                }
                targets
                    .iter()
                    .any(|target| BOOT_TARGETS.contains(&target.as_str()))
            }
            None => {
                assert!(
                    SERVING_WITHOUT_FILE
                        .iter()
                        .any(|(app, unit, _)| *app == owned.app && *unit == owned.unit),
                    "{}: a declared .service the scan holds no unit file for, and \
                     no SERVING_WITHOUT_FILE entry classifies it",
                    owned.id()
                );
                true
            }
        };
        if serving {
            out.insert(owned.app);
        }
    }
    out
}

/// The Apps at least one of whose roles reads its own service back:
/// [`probes_by_role`] folded through the role→App mapping.
fn probed_apps() -> BTreeSet<String> {
    probes_by_role()
        .keys()
        .filter_map(|role| app_of(role))
        .collect()
}

/// The presence fence (#720): an App that deploys a Serving Unit reads it, or
/// says out loud that it does not yet.
#[test]
fn test_every_app_with_a_serving_unit_reads_it_or_is_named() {
    let serving = serving_apps();
    let probed = probed_apps();
    let excused: BTreeSet<&str> = NOT_YET_PROBED
        .iter()
        .map(|(app, _)| *app)
        .chain(PROBED_BY_THE_INGRESS_GATE.iter().copied())
        .collect();

    let unprobed: Vec<&String> = serving
        .iter()
        .filter(|app| !probed.contains(*app) && !excused.contains(app.as_str()))
        .collect();
    assert_eq!(
        unprobed,
        Vec::<&String>::new(),
        "these Apps deploy a Serving Unit no role of theirs reads back, so a \
         broken deploy of one goes live unvalidated (#716). Add a readiness \
         probe (`wait_for`, or a loopback `uri`) after the flush, or join \
         NOT_YET_PROBED naming what the future probe would read"
    );
}

/// Both exception lists stay exact: an entry names an App that is actually in
/// the domain, and a debt entry leaves the moment its App gains a probe — the
/// ratchet only ever tightens.
#[test]
fn test_an_exception_names_a_serving_app_and_debt_leaves_by_probing() {
    let serving = serving_apps();
    let probed = probed_apps();
    for app in PROBED_BY_THE_INGRESS_GATE {
        assert!(
            serving.contains(*app),
            "{app} no longer deploys a Serving Unit; drop its \
             PROBED_BY_THE_INGRESS_GATE entry"
        );
    }
    for (app, _) in NOT_YET_PROBED {
        assert!(
            serving.contains(*app),
            "{app} no longer deploys a Serving Unit; drop its NOT_YET_PROBED entry"
        );
        assert!(
            !probed.contains(*app),
            "{app} gained a probe; drop its NOT_YET_PROBED entry so the fence \
             holds it probed from here on"
        );
    }
}

/// [`SERVING_WITHOUT_FILE`] entries stay underivable and declared, both
/// directions — the DECLARED_WITHOUT_FILE treatment: an entry the scan can
/// now classify is a hand answer shadowing a computed one, and an entry no
/// Meta declares is a claim about nothing.
#[test]
fn test_a_hand_classified_unit_stays_underivable_and_declared() {
    let declared = declared_units();
    let scanned: BTreeSet<String> = fleet_units()
        .iter()
        .filter(|unit| unit.dropin.is_none())
        .map(|unit| unit.name.clone())
        .collect();
    for (app, unit, _) in SERVING_WITHOUT_FILE {
        assert!(
            declared
                .iter()
                .any(|owned| owned.app == *app && owned.unit == *unit),
            "{app}/{unit} is hand-classified but no Meta declares it; drop the \
             stale entry"
        );
        assert!(
            !scanned.contains(*unit),
            "{app}/{unit} now has a unit file the scan reads; its [Install] \
             section is the classifier — drop the SERVING_WITHOUT_FILE entry"
        );
    }
}

/// The domain's lower boundary, pinned: an App whose units are all timer jobs
/// (or a packaged socket) is outside the domain and needs no exception entry,
/// because a timer-pulled service carries no `[Install]` boot target — the
/// timer activates it.
#[test]
fn test_a_timer_job_is_not_a_serving_unit() {
    let serving = serving_apps();
    for app in ["baikal", "cockpit", "colporteur"] {
        assert!(
            !serving.contains(app),
            "{app} was deliberately outside the presence domain; revisit the \
             Serving Unit classifier before enrolling it"
        );
    }
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
