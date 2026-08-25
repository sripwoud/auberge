//! Fleet-wide guard on the start rate limiter of every unit that restarts.
//!
//! systemd ships a start rate limiter on by default —
//! `DefaultStartLimitBurst=5` inside `DefaultStartLimitIntervalSec=10s` — and a
//! unit that trips it stops retrying and lands `failed`, where
//! `systemctl --failed` reports it. A unit that sets `RestartSec=5` makes that
//! default **unreachable by arithmetic**: four inter-start gaps of 5s already
//! span 20s, so the 10s window can never hold five starts however fast the App
//! fails. The unit then restarts without end, and because a unit in
//! auto-restart is `activating`, never `failed`, a hard-down App stays invisible
//! to the fleet's health signal for as long as it loops — grimmory did exactly
//! that for seven hours and 4628 failed starts on 2026-08-22 (#642).
//!
//! Whether a unit should give up at all is a judgement about what depends on
//! it, so this is ADR-0028's declared regime again: the units that restart are
//! computed from the fleet's own templates, the regime each one is in is
//! declared in `START_LIMIT_REGIMES`, and the limiter the repo writes for it is
//! matched against the regime's by equality in both directions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_yaml::{Mapping, Sequence, Value};

/// What systemd does with a unit that cannot start.
enum Limiter {
    /// The limiter is reachable: `burst` starts inside `interval_sec` end the
    /// retries and leave the unit `failed`.
    GivesUp { interval_sec: u64, burst: u32 },
    /// `StartLimitIntervalSec=0` — the limiter is off and the unit retries
    /// without end, deliberately.
    KeepsTrying,
}

/// A stance on giving up, shared by every unit that holds it. Named and
/// justified once rather than per unit, because the sizing follows from what
/// depends on the unit and not from the App behind it.
struct Regime {
    name: &'static str,
    limiter: Limiter,
    why: &'static str,
}

/// One hour and thirty starts. At the fleet's `RestartSec` of 5s or 10s that is
/// 145s to 290s of retrying before the verdict — long enough to outlast a
/// dependency that is `active` before it is ready (postgres recovering, mysql
/// replaying InnoDB, tailscaled negotiating, blocky not yet answering the names
/// an App resolves at startup), and far more attempts than a deterministic
/// failure needs, since attempt two already fails identically. Thirty also
/// leaves room for the deploys an operator runs back to back: `StartLimitBurst`
/// counts every start, not only the automatic ones, so a tight burst turns
/// iterative deployment into `start request repeated too quickly`.
static RESTARTING_APP: Regime = Regime {
    name: "Restarting App",
    limiter: Limiter::GivesUp {
        interval_sec: 3600,
        burst: 30,
    },
    why: "an App whose only health signal is `systemctl --failed`: retrying \
          forever hides it there, so it retries generously and then reports",
};

/// The one unit that must never stop trying. blocky is the Host's own resolver:
/// every other unit resolves names through it, so does the deploy that would
/// repair it, and so does the operator's own tooling. A terminal `failed` would
/// take away the path to recovery along with the App, and it buys least here for
/// the same reason — a resolver that cannot start is not silent, because the
/// names stop resolving.
static RESOLVER: Regime = Regime {
    name: "Resolver",
    limiter: Limiter::KeepsTrying,
    why: "what everything else resolves through, including its own repair, so \
          patience is worth more than a verdict nobody needs to be told",
};

/// Every unit the fleet restarts, and the regime it is in. Membership is
/// computed from the roles' own tasks and asserted against this list in both
/// directions, so a new restarting unit fails the build until it states which
/// regime it is in.
///
/// `navidrome.service` is the one entry the repo does not template: the `.deb`
/// ships the unit and the role writes a drop-in over it. Upstream sets
/// `StartLimitInterval=5` against `RestartSec=120`, which needs 18 minutes to
/// attempt its ten starts inside a five-second window — unreachable by a factor
/// of 216, and one this scan would never have seen from a template.
static START_LIMIT_REGIMES: &[(&str, &Regime)] = &[
    ("actual/actual.service", &RESTARTING_APP),
    ("bichon/bichon.service", &RESTARTING_APP),
    ("blocky/blocky.service", &RESOLVER),
    ("caddy/caddy.service", &RESTARTING_APP),
    ("calibre/calibre.service", &RESTARTING_APP),
    ("claude_code_remote/vibecoder.service", &RESTARTING_APP),
    ("freshrss/freshrss.service", &RESTARTING_APP),
    ("gokapi/gokapi.service", &RESTARTING_APP),
    ("grimmory/grimmory.service", &RESTARTING_APP),
    ("headscale/headscale.service", &RESTARTING_APP),
    ("hermes/hermes-gateway.service", &RESTARTING_APP),
    ("navidrome/navidrome.service", &RESTARTING_APP),
    ("paperless/paperless-consumer.service", &RESTARTING_APP),
    ("paperless/paperless-scheduler.service", &RESTARTING_APP),
    ("paperless/paperless-task-queue.service", &RESTARTING_APP),
    ("paperless/paperless-webserver.service", &RESTARTING_APP),
    ("radio/liquidsoap.service", &RESTARTING_APP),
    ("tgtg/tgtg.service", &RESTARTING_APP),
];

/// Every regime this fence defines. A regime defined and left out of here is
/// caught by `dead_code`; one listed and held by no unit is caught below.
static ALL_REGIMES: &[&Regime] = &[&RESTARTING_APP, &RESOLVER];

/// A unit the repo drops in over without templating it, and that needs no regime
/// because nothing restarts it. The repo holds no `Restart=` for such a unit, so
/// membership is this list's own claim and each entry carries what backs it.
static UNRESTARTED_ADOPTED_UNITS: &[(&str, &str)] = &[(
    "radio/icecast2.service",
    "Debian's icecast2 unit sets `Restart=no` — measured on auberge — so it \
     cannot crash-loop however it exits; the role drops in a Memory Budget and \
     nothing that would restart it",
)];

/// `Restart=`'s values, as systemd defines them. An unknown one is a hard stop
/// rather than a unit that quietly leaves the domain: systemd rejects a typo at
/// load, but this scan would read it as "does not restart" and stop asking the
/// unit for a regime.
const RESTART_VALUES: &[&str] = &[
    "no",
    "always",
    "on-success",
    "on-failure",
    "on-abnormal",
    "on-abort",
    "on-watchdog",
];

/// systemd's own default when a unit sets `Restart=` and no `RestartSec=`.
const DEFAULT_RESTART_SEC_MS: u64 = 100;

fn roles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles")
}

fn role_dir(role: &str) -> PathBuf {
    roles_dir().join(role)
}

fn all_roles() -> Vec<String> {
    let mut roles: Vec<String> = fs::read_dir(roles_dir())
        .expect("ansible/roles must exist")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    roles.sort();
    roles
}

fn field<'a>(task: &'a Mapping, key: &str) -> Option<&'a Value> {
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

fn flatten(tasks: &Sequence, out: &mut Vec<Mapping>) {
    for task in tasks {
        let Some(body) = task.as_mapping() else {
            continue;
        };
        let mut nested = false;
        for section in ["block", "rescue", "always"] {
            if let Some(inner) = field(body, section).and_then(Value::as_sequence) {
                flatten(inner, out);
                nested = true;
            }
        }
        if !nested {
            out.push(body.clone());
        }
    }
}

/// Every task in the role, across all of its task files. A unit installed under
/// a guard is still a unit whose starts systemd counts.
fn every_task(role: &str) -> Vec<Mapping> {
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
        flatten(&parsed, &mut tasks);
    }
    tasks
}

fn defaults(role: &str) -> BTreeMap<String, String> {
    let path = role_dir(role).join("defaults/main.yml");
    let Ok(raw) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let parsed: Mapping =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    parsed
        .iter()
        .filter_map(|(key, value)| {
            let key = key.as_str()?.to_string();
            match value {
                Value::String(text) => Some((key, text.clone())),
                Value::Number(number) => Some((key, number.to_string())),
                _ => None,
            }
        })
        .collect()
}

fn substitute(input: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = input;
    loop {
        let Some(open) = rest.find("{{") else {
            out.push_str(rest);
            return out;
        };
        let Some(offset) = rest[open..].find("}}") else {
            out.push_str(rest);
            return out;
        };
        let close = open + offset;
        match vars.get(rest[open + 2..close].trim()) {
            Some(value) => {
                out.push_str(&rest[..open]);
                out.push_str(value);
            }
            None => out.push_str(&rest[..close + 2]),
        }
        rest = &rest[close + 2..];
    }
}

/// `{{ var }}` replaced by the default it names, until the string stops
/// changing; anything the role's defaults do not state is left standing
/// verbatim.
fn resolve(raw: &str, vars: &BTreeMap<String, String>) -> String {
    let mut current = raw.to_string();
    for _ in 0..10 {
        let next = substitute(&current, vars);
        if next == current {
            return current;
        }
        current = next;
    }
    panic!("{raw} does not resolve to a fixed point");
}

/// What a `dest` configures, if anything this model reads: the unit it is or the
/// unit it refines. Drop-ins are in the domain here, unlike in ADR-0038's scan,
/// because a drop-in is how the repo reaches a unit it does not template — and
/// the fleet has one such unit that restarts.
///
/// A unit name still carrying an unresolved jinja expression is a hard stop: a
/// var-driven `loop:` this scan cannot expand would otherwise fail the
/// `.service` test and vanish from the domain silently, which is the one way a
/// new restarting unit could enter the fleet without entering the fence.
fn unit_configured_at(dest: &str) -> Option<(String, Option<String>)> {
    let (dir, file) = dest.rsplit_once('/')?;
    let in_unit_dir =
        |path: &str| path == "/etc/systemd/system" || path.ends_with("/.config/systemd/user");

    let (unit, dropin) = if in_unit_dir(dir) {
        (file.to_string(), None)
    } else {
        let (parent, unit_dir) = dir.rsplit_once('/')?;
        let unit = unit_dir.strip_suffix(".d")?;
        if !in_unit_dir(parent) || !file.ends_with(".conf") {
            return None;
        }
        (unit.to_string(), Some(file.to_string()))
    };

    if !unit.ends_with(".service") {
        return None;
    }
    assert!(
        !unit.contains("{{"),
        "`{dest}` configures a systemd unit whose name does not resolve; teach \
         this test how to expand it before relying on it"
    );
    Some((unit, dropin))
}

/// A systemd time span in milliseconds. The default unit for the settings this
/// test reads is seconds, and a span may be written as several components
/// (`1min 30s`). An unrecognised suffix is a hard stop: reading `5min` as 5
/// would make an unreachable limiter look reachable.
fn timespan_ms(unit: &str, key: &str, raw: &str) -> u64 {
    assert!(
        !raw.trim().is_empty(),
        "{unit} writes a bare `{key}=`, which resets the setting to systemd's \
         own default rather than to zero; this scan would read it as a \
         deliberate zero"
    );
    let mut total = 0u64;
    for component in raw.split_whitespace() {
        let digits = component
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let count: u64 = digits.parse().unwrap_or_else(|_| {
            panic!("{unit} writes `{key}={raw}`, which this test cannot read as a time span")
        });
        let multiplier = match &component[digits.len()..] {
            "" | "s" | "sec" | "second" | "seconds" => 1_000,
            "ms" | "msec" => 1,
            "m" | "min" | "minute" | "minutes" => 60_000,
            "h" | "hr" | "hour" | "hours" => 3_600_000,
            other => panic!(
                "{unit} writes `{key}={raw}`, whose `{other}` is a unit this test \
                 does not know; teach it the suffix before relying on it"
            ),
        };
        total += count * multiplier;
    }
    total
}

/// One assignment the repo makes for a unit: the section it is under, the key,
/// the value, and the file it came from.
struct Directive {
    section: String,
    key: String,
    value: String,
    file: String,
}

/// A unit the repo configures, and every assignment it makes for it — the unit
/// file first where the repo writes one, then drop-ins in the order systemd
/// loads them, so the last assignment is the effective one.
struct Unit {
    role: String,
    name: String,
    /// Whether the repo writes the unit file itself. A unit it only drops in
    /// over is packaged elsewhere, so its `Restart=` and `RestartSec=` are not
    /// in the repo to read.
    templated: bool,
    directives: Vec<Directive>,
}

impl Unit {
    fn id(&self) -> String {
        format!("{}/{}", self.role, self.name)
    }

    /// The value systemd would use: for the single-value settings this test
    /// reads, the last assignment in the named section wins.
    fn last_in(&self, section: &str, key: &str) -> Option<&str> {
        self.directives
            .iter()
            .filter(|d| d.section == section && d.key == key)
            .map(|d| d.value.as_str())
            .next_back()
    }

    /// Every `(file, section)` the key is assigned at, so a setting written
    /// where systemd does not read it can be named rather than silently
    /// believed.
    fn assignments_of(&self, key: &str) -> Vec<(&str, &str)> {
        self.directives
            .iter()
            .filter(|d| d.key == key)
            .map(|d| (d.file.as_str(), d.section.as_str()))
            .collect()
    }

    fn restart(&self) -> Option<&str> {
        let value = self.last_in("Service", "Restart")?;
        assert!(
            RESTART_VALUES.contains(&value),
            "{}: `{}` sets `Restart={value}`, which is not one of systemd's \
             values; this scan would read it as a unit that never restarts and \
             stop asking it for a start limit regime",
            self.role,
            self.name
        );
        Some(value)
    }

    /// Whether the repo's own files say this unit restarts. `false` for a unit
    /// the repo only drops in over: nothing in the repo says either way, which
    /// is why such a unit's regime is declared rather than computed.
    fn restarts(&self) -> bool {
        self.restart().is_some_and(|value| value != "no")
    }

    fn restart_sec_ms(&self) -> Option<u64> {
        self.last_in("Service", "RestartSec")
            .map(|raw| timespan_ms(&self.id(), "RestartSec", raw))
    }

    fn interval_ms(&self) -> Option<u64> {
        self.last_in("Unit", "StartLimitIntervalSec")
            .map(|raw| timespan_ms(&self.id(), "StartLimitIntervalSec", raw))
    }

    fn burst(&self) -> Option<u32> {
        self.last_in("Unit", "StartLimitBurst").map(|raw| {
            raw.parse().unwrap_or_else(|_| {
                panic!(
                    "{}: `{}` writes `StartLimitBurst={raw}`, which is not a count",
                    self.role, self.name
                )
            })
        })
    }
}

/// A unit file or drop-in parsed into its assignments. Comments and blank lines
/// are what an operator sees; only assignments matter here.
fn directives(body: &str, file: &str) -> Vec<Directive> {
    let mut section = String::new();
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            out.push(Directive {
                section: section.clone(),
                key: key.trim().to_string(),
                value: value.trim().to_string(),
                file: file.to_string(),
            });
        }
    }
    out
}

/// One file the repo installs for a unit: the unit itself, or a drop-in over it.
struct InstalledFile {
    unit: String,
    /// The drop-in's file name; `None` when the file is the unit itself.
    dropin: Option<String>,
    directives: Vec<Directive>,
}

/// Every unit file and drop-in the role installs.
fn installed_by(role: &str, vars: &BTreeMap<String, String>) -> Vec<InstalledFile> {
    let mut out = Vec::new();
    for task in every_task(role) {
        for module in ["ansible.builtin.template", "ansible.builtin.copy"] {
            let Some(args) = field(&task, module).and_then(Value::as_mapping) else {
                continue;
            };
            let dest = field(args, "dest").and_then(Value::as_str);
            let Some(src) = field(args, "src").and_then(Value::as_str) else {
                // A task with no `src` writes inline `content:`. Harmless for
                // anything but a unit, and a silent domain hole for a unit.
                if let Some(dest) = dest {
                    assert!(
                        unit_configured_at(&resolve(dest, vars)).is_none(),
                        "{role}: `{dest}` configures a systemd unit from inline \
                         `content:` rather than from a file; this scan reads \
                         `src` only, so teach it about the task before relying \
                         on it"
                    );
                }
                continue;
            };
            let Some(dest) = dest else {
                continue;
            };
            let items = strings(field(&task, "loop"));
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
                let Some((unit, dropin)) = unit_configured_at(&dest) else {
                    continue;
                };
                let file = src.rsplit('/').next().expect("a src names a file");
                let source = ["templates", "files"]
                    .iter()
                    .map(|dir| role_dir(role).join(dir).join(file))
                    .find(|path| path.is_file())
                    .unwrap_or_else(|| panic!("{role}: {file} is deployed but does not exist"));
                let body = fs::read_to_string(source).expect("a found source must be readable");
                let label = dropin.clone().unwrap_or_else(|| unit.clone());
                out.push(InstalledFile {
                    directives: directives(&body, &label),
                    unit,
                    dropin,
                });
            }
        }
    }
    out
}

fn units_of(role: &str) -> Vec<Unit> {
    let mut grouped: BTreeMap<String, Vec<InstalledFile>> = BTreeMap::new();
    for file in installed_by(role, &defaults(role)) {
        grouped.entry(file.unit.clone()).or_default().push(file);
    }
    grouped
        .into_iter()
        .map(|(name, mut files)| {
            // The unit file first, then drop-ins as systemd loads them: lexical
            // by file name, so the last assignment for a key is the live one.
            files.sort_by(|a, b| a.dropin.cmp(&b.dropin));
            Unit {
                role: role.to_string(),
                name,
                templated: files.iter().any(|file| file.dropin.is_none()),
                directives: files.into_iter().flat_map(|file| file.directives).collect(),
            }
        })
        .collect()
}

fn fleet_units() -> Vec<Unit> {
    let mut units: Vec<Unit> = all_roles().iter().flat_map(|role| units_of(role)).collect();
    units.sort_by(|a, b| (&a.role, &a.name).cmp(&(&b.role, &b.name)));
    units
}

fn declared() -> BTreeMap<&'static str, &'static Regime> {
    START_LIMIT_REGIMES.iter().copied().collect()
}

/// Computed -> declared. A unit that starts restarting fails until it states
/// which regime it is in, because systemd's default is neither regime: it is a
/// limiter no `RestartSec` in this fleet can reach.
#[test]
fn test_every_restarting_unit_the_repo_templates_declares_a_regime() {
    let listed: BTreeSet<String> = declared().keys().map(|id| id.to_string()).collect();
    let undeclared: Vec<String> = fleet_units()
        .iter()
        .filter(|unit| unit.templated && unit.restarts())
        .map(Unit::id)
        .filter(|id| !listed.contains(id))
        .collect();
    assert_eq!(
        undeclared,
        Vec::<String>::new(),
        "these units restart and declare no start limit regime; a unit that \
         restarts either gives up and reports or keeps trying on purpose, and \
         systemd's default is neither"
    );
}

/// Declared -> computed. Each entry is either a unit the repo templates and that
/// restarts, or a unit it only drops in over — for which the repo has no
/// `Restart=` to read, so membership is the declaration's own claim and the
/// least it must be backed by is a drop-in that exists. Without this the table
/// could name anything and every assertion below would skip it.
#[test]
fn test_every_declared_unit_is_one_the_repo_configures() {
    let scanned = fleet_units();
    let by_id: BTreeMap<String, &Unit> = scanned.iter().map(|unit| (unit.id(), unit)).collect();
    for (id, _) in declared() {
        let Some(unit) = by_id.get(id) else {
            panic!(
                "START_LIMIT_REGIMES declares a regime for `{id}`, which no role \
                 installs a unit file or drop-in for — either it is gone or the \
                 scan stopped seeing it, and the second is the dangerous one"
            );
        };
        if unit.templated {
            assert!(
                unit.restarts(),
                "{id} is declared but its unit file sets `Restart={}`; a limiter \
                 on a unit nothing restarts only stands to refuse a start \
                 somebody meant",
                unit.restart().unwrap_or("no")
            );
        }
    }
}

/// Declared -> written. The regime is the decision; the two directives are how
/// systemd is told about it, and a decision the repo's files do not carry is a
/// decision that does not happen.
#[test]
fn test_every_unit_carries_the_limiter_its_regime_declares() {
    let declared = declared();
    for unit in fleet_units() {
        let Some(regime) = declared.get(unit.id().as_str()) else {
            continue;
        };
        match regime.limiter {
            Limiter::GivesUp {
                interval_sec,
                burst,
            } => {
                assert_eq!(
                    unit.interval_ms(),
                    Some(interval_sec * 1_000),
                    "{}: `{}` is a {} — {} — so it must set \
                     `StartLimitIntervalSec={interval_sec}` under `[Unit]`",
                    unit.role,
                    unit.name,
                    regime.name,
                    regime.why
                );
                assert_eq!(
                    unit.burst(),
                    Some(burst),
                    "{}: `{}` is a {}, so it must set `StartLimitBurst={burst}` \
                     under `[Unit]`; systemd's default of 5 is not this regime's \
                     number and would change under it",
                    unit.role,
                    unit.name,
                    regime.name
                );
            }
            Limiter::KeepsTrying => {
                assert_eq!(
                    unit.interval_ms(),
                    Some(0),
                    "{}: `{}` is a {} — {} — so it must turn the limiter off \
                     with `StartLimitIntervalSec=0` under `[Unit]`",
                    unit.role,
                    unit.name,
                    regime.name,
                    regime.why
                );
                assert_eq!(
                    unit.burst(),
                    None,
                    "{}: `{}` turns the limiter off, so a `StartLimitBurst` is a \
                     count nothing counts to; drop it rather than leave two \
                     readings of one decision",
                    unit.role,
                    unit.name
                );
            }
        }
    }
}

/// A unit the repo only drops in over has no `Restart=` in the repo to read, so
/// it cannot enter the domain by computation the way a templated unit does. Each
/// one is classified by hand instead, and a new drop-in over a packaged unit
/// fails the build until it lands on one of the two lists. Without this the
/// navidrome class is unfenced — which is why the fleet's most unreachable
/// limiter was found by reading a unit on the Host rather than by this test.
#[test]
fn test_every_unit_the_repo_only_drops_in_over_is_classified() {
    let declared = declared();
    let unrestarted: BTreeSet<&str> = UNRESTARTED_ADOPTED_UNITS
        .iter()
        .map(|(id, _)| *id)
        .collect();
    let adopted: BTreeSet<String> = fleet_units()
        .iter()
        .filter(|unit| !unit.templated)
        .map(Unit::id)
        .collect();

    for id in &adopted {
        let regime = declared.contains_key(id.as_str());
        let unrestarting = unrestarted.contains(id.as_str());
        assert!(
            regime || unrestarting,
            "{id} is a unit this repo drops in over but does not template, so \
             nothing here says whether it restarts; give it a Start Limit Regime \
             or list it in UNRESTARTED_ADOPTED_UNITS with what backs that"
        );
        assert!(
            !(regime && unrestarting),
            "{id} is declared both as holding a regime and as never restarting; \
             one of the two is wrong and neither says which"
        );
    }
    for (id, _) in UNRESTARTED_ADOPTED_UNITS {
        assert!(
            adopted.contains(*id),
            "UNRESTARTED_ADOPTED_UNITS vouches for `{id}`, which this repo no \
             longer drops in over; drop the entry with the last task that did"
        );
    }
}

/// A unit the repo does not template has its `RestartSec` set by whoever
/// packaged it, and the arithmetic below is measured against that value. The
/// drop-in has to pin it, or the fence would be vouching for a number that can
/// change under an upstream bump with nothing in the repo to notice — navidrome
/// ships `RestartSec=120`, which needs 58 minutes to spend a burst of 30.
#[test]
fn test_an_adopted_units_dropin_pins_the_restart_delay_it_is_judged_against() {
    let declared = declared();
    for unit in fleet_units().iter().filter(|unit| !unit.templated) {
        if !declared.contains_key(unit.id().as_str()) {
            continue;
        }
        assert!(
            unit.restart_sec_ms().is_some(),
            "{}: `{}` is packaged elsewhere, so its `RestartSec` is not in this \
             repo; its drop-in must set one, because that is the number the \
             limiter's reach is computed from",
            unit.role,
            unit.name
        );
    }
}

/// The arithmetic, read off the repo's files rather than off the declaration, so
/// the fence still holds when a regime's numbers are edited: `burst` starts are
/// separated by at least `burst - 1` gaps of `RestartSec`, and a window shorter
/// than that can never hold them however fast the App fails. This is the exact
/// shape systemd's defaults had on 15 of the fleet's units, and the reason
/// grimmory restarted 4628 times without ever being reported.
#[test]
fn test_a_limiter_that_gives_up_can_admit_its_burst_before_its_window_closes() {
    for unit in fleet_units() {
        let (Some(interval_ms), Some(burst)) = (unit.interval_ms(), unit.burst()) else {
            continue;
        };
        if interval_ms == 0 {
            continue;
        }
        assert!(
            burst >= 2,
            "{}: `{}` sets `StartLimitBurst={burst}`; 0 turns the limiter off by \
             another name and 1 gives up before a single retry — neither is a \
             regime this fence models",
            unit.role,
            unit.name
        );
        let restart_sec_ms = unit.restart_sec_ms().unwrap_or(DEFAULT_RESTART_SEC_MS);
        let span_ms = u64::from(burst - 1) * restart_sec_ms;
        assert!(
            span_ms < interval_ms,
            "{}: `{}` needs {span_ms}ms to attempt {burst} starts at `RestartSec` \
             {restart_sec_ms}ms, inside a window of {interval_ms}ms — the limiter \
             cannot be reached however fast the App fails, so the unit restarts \
             without end and never reaches `systemctl --failed`",
            unit.role,
            unit.name
        );
    }
}

/// Measured on auberge under systemd 257, with a transient unit per case:
/// `StartLimitBurst` is honoured from `[Service]`, and `StartLimitIntervalSec`
/// is not — it is read only from `[Unit]`, and only the legacy spelling
/// `StartLimitInterval` survives in `[Service]`. So the pair splits, and each
/// half fails differently; the consequence is stated per key rather than once.
const OUTSIDE_UNIT_SECTION: &[(&str, &str)] = &[
    (
        "StartLimitIntervalSec",
        "systemd does not read it outside `[Unit]` at all, so the window would \
         silently stay its 10s default while the file claims otherwise",
    ),
    (
        "StartLimitBurst",
        "systemd does honour it there, which is the worse half: the burst takes \
         effect beside a window that stayed at the default, and that split pair \
         is exactly what an unreachable limiter looks like when someone believes \
         they configured it",
    ),
];

#[test]
fn test_no_start_limit_setting_sits_outside_the_unit_section() {
    for unit in fleet_units() {
        for (key, consequence) in OUTSIDE_UNIT_SECTION {
            let stray: Vec<(&str, &str)> = unit
                .assignments_of(key)
                .into_iter()
                .filter(|(_, section)| *section != "Unit")
                .collect();
            assert!(
                stray.is_empty(),
                "{}: `{}` assigns `{key}` at {stray:?}; {consequence}",
                unit.role,
                unit.name
            );
        }
    }
}

/// `StartLimitInterval` without the `Sec` is the pre-v229 spelling of the same
/// setting, and systemd still honours it — from `[Service]`, where the modern
/// name does nothing. A unit written that way is configured and this scan cannot
/// see it, which is worse than one that is misconfigured and can.
#[test]
fn test_no_start_limit_setting_uses_the_legacy_spelling() {
    for unit in fleet_units() {
        let legacy = unit.assignments_of("StartLimitInterval");
        assert!(
            legacy.is_empty(),
            "{}: `{}` writes `StartLimitInterval` at {legacy:?}; that is the \
             pre-v229 name for `StartLimitIntervalSec`, systemd honours it, and \
             this scan reads only the modern spelling",
            unit.role,
            unit.name
        );
    }
}

/// A unit with no declared regime must carry no limiter from this repo. Either
/// it does not restart — in which case the limiter only stands to refuse a start
/// its timer meant — or it does, and it belongs in the table.
#[test]
fn test_no_undeclared_unit_limits_its_starts() {
    let declared = declared();
    for unit in fleet_units() {
        if declared.contains_key(unit.id().as_str()) {
            continue;
        }
        assert_eq!(
            (unit.interval_ms(), unit.burst()),
            (None, None),
            "{}: `{}` limits its starts and declares no regime; nothing in \
             START_LIMIT_REGIMES says whether it should give up or keep trying",
            unit.role,
            unit.name
        );
    }
}

/// The declared regimes have to stay live: a stance no unit holds any more is a
/// claim about the fleet nobody checks.
#[test]
fn test_every_declared_regime_is_still_held_by_a_unit() {
    let held: BTreeSet<&str> = declared().values().map(|regime| regime.name).collect();
    for regime in ALL_REGIMES {
        assert!(
            held.contains(regime.name),
            "the {} regime is declared but no unit is in it; drop it with the \
             last unit that was",
            regime.name
        );
    }
}
