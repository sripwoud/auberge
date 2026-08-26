//! Fleet-wide guard on the exit status a clean shutdown leaves behind.
//!
//! systemd is lenient about a service *dying from* SIGTERM — `code=killed,
//! signal=TERM` scores as success without any declaration. It is not lenient
//! about a service that *catches* SIGTERM, runs its own shutdown, and then
//! exits 128+15 of its own accord: that arrives as `code=exited, status=143`
//! and scores as a failure. A unit whose runtime shuts down that way therefore
//! latches `failed` on every deliberate stop — which is what the Backup Recipe
//! does nightly, and what `systemctl --failed` is watched for (#635).
//!
//! Whether a runtime exits or dies is not something the repo can read off a
//! template, so this is ADR-0028's declared regime again: the runtime each
//! unit execs is computed from its `ExecStart`, the status a clean shutdown
//! produces is declared in `CLEAN_SHUTDOWN_EXITS`, and `SuccessExitStatus=` is
//! matched against the pairing by equality in both directions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde_yaml::Value;

mod common;

use common::{all_roles, defaults, field, resolve, role_dir, role_tasks, strings};

/// A runtime that exits with a nonzero status on a clean shutdown, and the
/// status it exits with. Keyed by the runtime rather than by the unit, so the
/// next App built on one of these enters the fence without being enrolled.
struct CleanShutdownExit {
    /// `ExecStart`'s argv[0], by basename.
    runtime: &'static str,
    status: u16,
    why: &'static str,
}

const CLEAN_SHUTDOWN_EXITS: &[CleanShutdownExit] = &[CleanShutdownExit {
    runtime: "java",
    status: 143,
    why: "the JVM installs a SIGTERM handler, runs its shutdown hooks, and then \
          exits 128+SIGTERM itself rather than dying from the signal, so systemd \
          sees an exit status where it would have forgiven a signal death",
}];

/// A `.service` a role installs, whether under `/etc/systemd/system` or a
/// user's `~/.config/systemd/user`. Drop-ins are excluded: they refine a unit
/// installed by something else, and the `ExecStart` they would be judged
/// against is not theirs.
struct ServiceUnit {
    role: String,
    name: String,
    /// `ExecStart`'s argv[0] by basename, resolved.
    runtime: String,
    /// Every `SuccessExitStatus=` token, resolved; systemd merges repeated
    /// lines, so this does too.
    declared: BTreeSet<String>,
    /// `Restart=`, or `no` where the unit leaves it out, as systemd defaults it.
    restart: String,
}

/// The unit name a `dest` installs, if that dest is a `.service` in a systemd
/// unit directory this model reads — the system directory or a user's. A
/// drop-in's dest lands under `<unit>.service.d/`, so it is excluded by the
/// same test that admits a unit.
///
/// A file name that still carries an unresolved jinja expression is a hard
/// stop: a var-driven `loop:` this scan cannot expand would otherwise fail the
/// `.service` test and vanish from the domain silently, which is the one way a
/// new unit could enter the fleet without entering the fence. An unresolved
/// expression in the *directory* is not that case — hermes installs a user
/// unit under an admin home the role's defaults do not name, and the unit it
/// installs there is still named in full.
fn service_installed_at(dest: &str) -> Option<&str> {
    let (dir, file) = dest.rsplit_once('/')?;
    if dir != "/etc/systemd/system" && !dir.ends_with("/.config/systemd/user") {
        return None;
    }
    assert!(
        !file.contains("{{"),
        "`{dest}` installs into a systemd unit directory under a name that does \
         not resolve; teach this test how to expand it before relying on it"
    );
    file.ends_with(".service").then_some(file)
}

/// argv[0]'s basename from an `ExecStart=` value. systemd's special prefixes
/// (`-`, `@`, `+`, `!`) change what argv[0] means, and nothing in the fleet
/// uses one, so meeting one is a hard stop rather than a silent misread.
fn runtime_of(role: &str, unit: &str, exec_start: &str) -> String {
    let argv0 = exec_start
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("{role}: `{unit}` has an empty ExecStart"));
    assert!(
        !argv0.starts_with(['-', '@', '+', '!']),
        "{role}: `{unit}` prefixes ExecStart with `{argv0}`; systemd's exec \
         prefixes change what argv[0] is, so teach this test about it before \
         relying on it"
    );
    argv0
        .rsplit('/')
        .next()
        .expect("splitting a nonempty string yields a segment")
        .to_string()
}

fn service_units(role: &str, vars: &BTreeMap<String, String>) -> Vec<ServiceUnit> {
    let mut units = Vec::new();
    for task in role_tasks(role) {
        for module in ["ansible.builtin.template", "ansible.builtin.copy"] {
            let Some(args) = field(&task.body, module).and_then(Value::as_mapping) else {
                continue;
            };
            let (Some(dest), Some(src)) = (
                field(args, "dest").and_then(Value::as_str),
                field(args, "src").and_then(Value::as_str),
            ) else {
                continue;
            };
            let items = strings(field(&task.body, "loop"));
            let expansions: Vec<(String, String)> = if items.is_empty() {
                vec![(dest.to_string(), src.to_string())]
            } else {
                items
                    .iter()
                    .map(|item| {
                        (
                            dest.replace("{{ item }}", item),
                            src.replace("{{ item }}", item),
                        )
                    })
                    .collect()
            };
            for (dest, src) in expansions {
                let dest = resolve(&dest, vars);
                let Some(name) = service_installed_at(&dest) else {
                    continue;
                };
                let name = name.to_string();
                let file = src.rsplit('/').next().expect("a src names a file");
                let template = ["templates", "files"]
                    .iter()
                    .map(|dir| role_dir(role).join(dir).join(file))
                    .find(|path| path.is_file())
                    .unwrap_or_else(|| panic!("{role}: {file} is deployed but does not exist"));
                let body = fs::read_to_string(template).expect("a found template must be readable");
                let execs: Vec<&str> = body
                    .lines()
                    .filter_map(|line| line.strip_prefix("ExecStart="))
                    .collect();
                let [exec_start] = execs[..] else {
                    panic!(
                        "{role}: `{name}` declares {} ExecStart lines; one runtime \
                         per unit is what this model reads, so teach it about the \
                         rest before relying on it",
                        execs.len()
                    )
                };
                units.push(ServiceUnit {
                    runtime: runtime_of(role, &name, &resolve(exec_start.trim(), vars)),
                    restart: body
                        .lines()
                        .filter_map(|line| line.strip_prefix("Restart="))
                        .next_back()
                        .unwrap_or("no")
                        .trim()
                        .to_string(),
                    declared: body
                        .lines()
                        .filter_map(|line| line.strip_prefix("SuccessExitStatus="))
                        .flat_map(|value| {
                            resolve(value.trim(), vars)
                                .split([' ', '\t', ','])
                                .filter(|token| !token.is_empty())
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .collect(),
                    role: role.to_string(),
                    name,
                });
            }
        }
    }
    units
}

fn fleet_units() -> Vec<ServiceUnit> {
    let mut units: Vec<ServiceUnit> = all_roles()
        .iter()
        .flat_map(|role| service_units(role, &defaults(role)))
        .collect();
    units.sort_by(|a, b| (&a.role, &a.name).cmp(&(&b.role, &b.name)));
    units
}

fn declared_for(runtime: &str) -> Option<&'static CleanShutdownExit> {
    CLEAN_SHUTDOWN_EXITS
        .iter()
        .find(|entry| entry.runtime == runtime)
}

/// Every service the fleet installs, `<role>/<unit>` so a rename shows on
/// both sides. The scan's own reach, pinned as a set rather than a count: a
/// unit added while another is dropped keeps a count green, and a count cannot
/// name which unit moved.
const FLEET_SERVICES: &[&str] = &[
    "actual/actual.service",
    "baikal/baikal-birthday-sync.service",
    "baikal/baikal-busy-sync.service",
    "bichon/bichon-archive.service",
    "bichon/bichon-uidvalidity-watch.service",
    "bichon/bichon.service",
    "blocky/blocky.service",
    "blocky/lego-renew.service",
    "caddy/caddy.service",
    "calibre/calibre.service",
    "claude_code_remote/vibecoder.service",
    "colporteur/colporteur.service",
    "freshrss/freshrss-update.service",
    "freshrss/freshrss.service",
    "gokapi/gokapi.service",
    "grimmory/grimmory.service",
    "headscale/headscale.service",
    "hermes/hermes-gateway.service",
    "immich/immich-backup.service",
    "immich/immich.service",
    "paperless/paperless-consumer.service",
    "paperless/paperless-scheduler.service",
    "paperless/paperless-task-queue.service",
    "paperless/paperless-webserver.service",
    "radio/liquidsoap.service",
    "tgtg/tgtg.service",
];

/// The scan's reach, by equality in both directions: a new service fails until
/// it is listed, and a listing the fleet no longer installs fails until it is
/// removed. Without it every assertion below can pass by seeing nothing.
#[test]
fn test_the_scan_sees_exactly_the_services_the_fleet_installs() {
    let seen: BTreeSet<String> = fleet_units()
        .iter()
        .map(|unit| format!("{}/{}", unit.role, unit.name))
        .collect();
    let listed: BTreeSet<String> = FLEET_SERVICES.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        seen.difference(&listed).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "the scan found services FLEET_SERVICES does not list; every unit has a \
         shutdown systemd scores, so add them"
    );
    assert_eq!(
        listed.difference(&seen).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "FLEET_SERVICES lists services the scan no longer finds; either the unit \
         is gone or the scan stopped seeing it — the second is the dangerous one"
    );
}

/// Computed -> declared. A unit whose runtime exits nonzero on a clean
/// shutdown must say so, or the Backup Recipe's nightly stop latches it
/// `failed` and `systemctl --failed` stops meaning anything.
#[test]
fn test_a_unit_whose_runtime_exits_on_sigterm_declares_that_status() {
    for unit in fleet_units() {
        let Some(exit) = declared_for(&unit.runtime) else {
            continue;
        };
        assert_eq!(
            unit.declared,
            BTreeSet::from([exit.status.to_string()]),
            "{}: `{}` execs `{}` and {}, so a clean stop arrives as \
             status={} — declare `SuccessExitStatus={}` or every deliberate \
             stop scores as a unit failure",
            unit.role,
            unit.name,
            unit.runtime,
            exit.why,
            exit.status,
            exit.status
        );
    }
}

/// Declared -> computed, and by exact value. A whitelist wider than the one
/// status the runtime's clean shutdown produces would forgive a real crash:
/// the 4628 `status=1` exits grimmory logged inside one seven-hour window on
/// 2026-08-22 are exactly what has to keep registering as failed.
#[test]
fn test_no_unit_forgives_a_status_its_runtime_does_not_produce() {
    for unit in fleet_units() {
        if unit.declared.is_empty() {
            continue;
        }
        assert!(
            declared_for(&unit.runtime).is_some(),
            "{}: `{}` forgives {:?} but execs `{}`, which no entry in \
             CLEAN_SHUTDOWN_EXITS says exits nonzero on a clean shutdown; a \
             status forgiven for no stated reason is a crash nobody sees",
            unit.role,
            unit.name,
            unit.declared,
            unit.runtime
        );
    }
}

/// The declared table itself has to stay live: a runtime no unit execs any
/// more is a claim about the fleet nobody checks.
#[test]
fn test_every_declared_runtime_is_still_exec_ed_by_a_unit() {
    let running: BTreeSet<String> = fleet_units().into_iter().map(|unit| unit.runtime).collect();
    for entry in CLEAN_SHUTDOWN_EXITS {
        assert!(
            running.contains(entry.runtime),
            "CLEAN_SHUTDOWN_EXITS declares `{}` but no fleet unit execs it; \
             drop the entry with the last App that did",
            entry.runtime
        );
    }
}

/// Forgiving a status takes it out of `Restart=on-failure`'s trigger set, so
/// the two cannot be combined: a SIGTERM from anywhere but systemd would leave
/// the unit stopped and clean, invisible to `systemctl --failed` as well as to
/// the restart that used to recover it. `always` restores that recovery while
/// still honouring a deliberate stop, and leaving `Restart` out is the honest
/// answer for a unit nothing should revive.
#[test]
fn test_forgiving_a_status_does_not_leave_on_failure_watching_for_it() {
    for unit in fleet_units() {
        if unit.declared.is_empty() {
            continue;
        }
        assert_ne!(
            unit.restart, "on-failure",
            "{}: `{}` forgives {:?} and asks `Restart=on-failure` to recover \
             from it — the two cancel out. An external SIGTERM would leave the \
             unit dead, clean, and unreported; use `Restart=always`",
            unit.role, unit.name, unit.declared
        );
    }
}
