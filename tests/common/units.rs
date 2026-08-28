//! The systemd units the ansible tree installs, as the fences read them.
//!
//! Five fences ask what amounts to one question — which unit does this role
//! install, and what does its file say — and each carried its own answer:
//! `start_limit`, `service_directories`, `install_notifies_restart`,
//! `shutdown_exit_status` and `unit_ownership`. Four pieces were duplicated
//! near-identically across them: the `dest`→unit resolver, the
//! `template`/`copy` scan with its `{{ item }}` loop expansion, the resolution
//! of a `src` to a file under `templates/` or `files/`, and the reading of the
//! unit file's directives. `tests/common/mod.rs` is the same lesson one layer
//! down, on the task walk these all read the tree through (#654, #668).
//!
//! The copies had diverged in the way that does not fail. Only `start_limit`'s
//! reader tracked `[Unit]`/`[Service]` sections; the other three read a
//! directive with `line.strip_prefix("Restart=")` over every line in the file,
//! so an assignment under `[Install]` or `[Timer]` — where systemd applies
//! nothing — would have read as live `[Service]` configuration. A fence
//! satisfied by a directive systemd never applies passes vacuously, which is
//! the failure mode this module exists to remove: [`directives`] is
//! `start_limit`'s parser, and it is now the only one.
//!
//! One [`InstalledUnit`] is one *file*, not one unit: the unit file and each
//! drop-in over it stay separate, because that difference is load-bearing —
//! `start_limit` asserts that an adopted unit's drop-in pins the `RestartSec`
//! the limiter's reach is computed against, which is a claim about which file
//! an assignment is in. A fence wanting systemd's effective view merges the
//! files in the order systemd loads them, and `start_limit` is the one that
//! does.
//!
//! The domain here is the *union* of what the five fences read, and each picks
//! its own subset at the call site, the way `common/mod.rs`'s walks are picked:
//!
//! - `unit_ownership` takes all of it — every unit type, both managers,
//!   drop-ins included, since a drop-in names the unit it refines.
//! - `start_limit` takes `.service` files and their drop-ins. Only a service
//!   has a `Restart=` to limit; a `.timer` or `.socket` in that domain would be
//!   a unit the fence can only ever pass on.
//! - `shutdown_exit_status` takes `.service` unit files, without drop-ins: a
//!   drop-in's `ExecStart` is not the one it would be judged against.
//! - `install_notifies_restart` and `service_directories` take system-manager
//!   unit files, without drop-ins — what apt packaged is outside both models.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use auberge::playbook_meta::UNIT_TYPE_SUFFIXES;
use serde_yaml::{Mapping, Value};

use super::{all_roles, defaults, field, relative, resolve, role_dir, role_tasks, strings};

/// The systemd manager a unit is loaded by. Which one decides where the file
/// lands, and nothing else here reads it — but `unit_ownership` declares it per
/// unit in the App's Meta, because `systemctl` needs `--user` to address one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Scope {
    System,
    User,
}

/// One assignment a unit file or drop-in makes.
///
/// The section is carried rather than discarded because systemd reads a key
/// only where it is defined to: `StartLimitIntervalSec` is honoured from
/// `[Unit]` and nowhere else, `StartLimitBurst` from `[Unit]` *and* `[Service]`,
/// and everything under `[Install]` is read by `systemctl enable` rather than at
/// load. A reader that flattens the file cannot tell those apart.
///
/// The value is the file's own text, unresolved. A fence that needs `{{ … }}`
/// substituted calls [`super::resolve`] with its role's defaults at the read
/// site, which is where it knows whether an unresolved expression is a fact or
/// a failure — `start_limit` reads a number that never holds one, while
/// `service_directories` fails loud on a grant it cannot resolve to a path.
pub struct Directive {
    pub section: String,
    pub key: String,
    pub value: String,
}

/// One file the repo installs for a systemd unit: the unit file itself, or a
/// drop-in over it.
pub struct InstalledUnit {
    pub role: String,
    /// The unit as `systemctl` addresses it, `caddy.service`. For a drop-in,
    /// the unit its `<unit>.d/` directory names.
    pub name: String,
    pub scope: Scope,
    /// The drop-in's file name; `None` when this file is the unit itself.
    pub dropin: Option<String>,
    /// Every assignment this one file makes, in file order.
    pub directives: Vec<Directive>,
}

impl InstalledUnit {
    /// This one file's identity, drop-in path and manager included — the form
    /// [`FLEET_UNIT_FILES`] pins the scan's reach by.
    ///
    /// Two files for one unit differ here and nowhere else, which is what the
    /// pin needs; a fence keyed by the *unit* spells `<role>/<name>` itself,
    /// because the two that do carry a merged view of their own.
    pub fn file_id(&self) -> String {
        let scope = match self.scope {
            Scope::System => "",
            Scope::User => " (user)",
        };
        match &self.dropin {
            Some(conf) => format!("{}/{}.d/{conf}{scope}", self.role, self.name),
            None => format!("{}/{}{scope}", self.role, self.name),
        }
    }

    /// The file name an assignment came from, for a message pointing at where
    /// to edit.
    pub fn file(&self) -> &str {
        self.dropin.as_deref().unwrap_or(&self.name)
    }

    /// Every assignment of `key` under `section`, in file order. systemd merges
    /// repeated lines for the list-valued settings — `ReadWritePaths`,
    /// `SuccessExitStatus`, `Environment` — so a caller reading one of those
    /// wants all of them.
    pub fn all_in(&self, section: &str, key: &str) -> Vec<&str> {
        self.directives
            .iter()
            .filter(|d| d.section == section && d.key == key)
            .map(|d| d.value.as_str())
            .collect()
    }

    /// The value systemd would use for a single-value setting: the last
    /// assignment in the section wins.
    pub fn last_in(&self, section: &str, key: &str) -> Option<&str> {
        self.all_in(section, key).pop()
    }

    /// Every section `key` is assigned under, so a setting written where
    /// systemd does not read it can be named rather than silently believed.
    pub fn sections_assigning(&self, key: &str) -> Vec<&str> {
        self.directives
            .iter()
            .filter(|d| d.key == key)
            .map(|d| d.section.as_str())
            .collect()
    }
}

/// A unit file or drop-in parsed into its assignments, section by section.
/// Comments and blank lines are what an operator sees; only assignments matter
/// here.
pub fn directives(body: &str) -> Vec<Directive> {
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
            });
        }
    }
    out
}

/// The unit a `dest` configures, if it lands in a systemd unit directory this
/// model reads — the system manager's, or a user's — either as the unit file
/// itself or as a drop-in under `<unit>.d/`, which names the unit it refines
/// just as well.
///
/// The *directory* may hold an unresolved expression and often does: hermes
/// installs its user unit under an admin home the role's defaults do not name.
/// The unit *name* may not. A var-driven `loop:` this scan cannot expand would
/// otherwise fail the suffix test below and vanish from the domain silently,
/// which is the one way a new unit could enter the fleet without entering any
/// of the five fences.
pub fn unit_configured_at(dest: &str) -> Option<(String, Scope, Option<String>)> {
    let (dir, file) = dest.rsplit_once('/')?;
    let scope_of = |path: &str| {
        if path == "/etc/systemd/system" {
            Some(Scope::System)
        } else if path.ends_with("/.config/systemd/user") {
            Some(Scope::User)
        } else {
            None
        }
    };

    let (unit, scope, dropin) = match scope_of(dir) {
        Some(scope) => (file.to_string(), scope, None),
        None => {
            let (parent, unit_dir) = dir.rsplit_once('/')?;
            let unit = unit_dir.strip_suffix(".d")?;
            let scope = scope_of(parent)?;
            if !file.ends_with(".conf") {
                return None;
            }
            (unit.to_string(), scope, Some(file.to_string()))
        }
    };

    assert!(
        !unit.contains("{{"),
        "`{dest}` configures a systemd unit whose name does not resolve; teach \
         this scan how to expand it before relying on it"
    );
    UNIT_TYPE_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
        .then_some((unit, scope, dropin))
}

/// Where a task's bytes come from. A `copy` may write a file it names with
/// `src:` or one it spells out inline with `content:`, and both install real
/// units — cockpit's `cockpit.socket` drop-in is written inline, and is the
/// reason this is a choice rather than a `src` requirement.
enum Body {
    /// `src:` — a file under the role's `templates/` or `files/`.
    File(String),
    /// `content:` — the file body written out in the task itself.
    Inline(String),
}

impl Body {
    fn with_item(&self, item: &str) -> Body {
        match self {
            Body::File(src) => Body::File(src.replace("{{ item }}", item)),
            Body::Inline(text) => Body::Inline(text.replace("{{ item }}", item)),
        }
    }

    /// The file's text, read out of the role.
    ///
    /// A `src` the role does not hold is a hard stop rather than an absent
    /// unit, which would leave the domain one short. That the deploy would
    /// fail on it too holds only for a `src` ansible reads locally:
    /// `remote_src: true` names a path on the Host, so such a task is valid
    /// and its body is not in the repo at all. [`installed_by`] stops on one
    /// before reaching here, with the reason it can actually give.
    fn text(&self, role: &str) -> String {
        match self {
            Body::Inline(text) => text.clone(),
            Body::File(src) => {
                let file = src.rsplit('/').next().expect("a src names a file");
                let path = ["templates", "files"]
                    .iter()
                    .map(|dir| role_dir(role).join(dir).join(file))
                    .find(|path| path.is_file())
                    .unwrap_or_else(|| panic!("{role}: {file} is deployed but does not exist"));
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)))
            }
        }
    }
}

fn body_of(args: &Mapping) -> Option<Body> {
    if let Some(src) = field(args, "src").and_then(Value::as_str) {
        return Some(Body::File(src.to_string()));
    }
    field(args, "content")
        .and_then(Value::as_str)
        .map(|text| Body::Inline(text.to_string()))
}

/// Every unit file and drop-in one role's tasks install.
///
/// `loop:` is the only iteration expanded, because it is the only one the fleet
/// writes a unit under; the two `with_items:` in the tree are on
/// `ansible.builtin.file` tasks, which install no unit. A `with_items:` on a
/// `template` writing into a unit directory would be a hole, and it is one this
/// scan does not have yet rather than one it papers over.
fn installed_by(role: &str, vars: &BTreeMap<String, String>) -> Vec<InstalledUnit> {
    let mut out = Vec::new();
    for task in role_tasks(role) {
        for module in ["ansible.builtin.template", "ansible.builtin.copy"] {
            let Some(args) = field(&task.body, module).and_then(Value::as_mapping) else {
                continue;
            };
            let (Some(dest), Some(body)) =
                (field(args, "dest").and_then(Value::as_str), body_of(args))
            else {
                continue;
            };
            let items = strings(field(&task.body, "loop"));
            let expansions: Vec<(String, Body)> = if items.is_empty() {
                vec![(dest.to_string(), body)]
            } else {
                items
                    .iter()
                    .map(|item| (dest.replace("{{ item }}", item), body.with_item(item)))
                    .collect()
            };
            for (dest, body) in expansions {
                let Some((name, scope, dropin)) = unit_configured_at(&resolve(&dest, vars)) else {
                    continue;
                };
                assert!(
                    field(args, "remote_src")
                        .and_then(Value::as_bool)
                        .is_none_or(|remote| !remote),
                    "{role}: `{dest}` installs a systemd unit from a `remote_src`, \
                     so its body is a path on the Host and not in this repo; teach \
                     this scan where to read it before relying on it"
                );
                out.push(InstalledUnit {
                    role: role.to_string(),
                    name,
                    scope,
                    dropin,
                    directives: directives(&body.text(role)),
                });
            }
        }
    }
    out
}

/// Every unit file and drop-in the fleet installs, by [`InstalledUnit::file_id`].
///
/// The scan's reach, by equality in both directions: a new unit file fails
/// until it is listed, and a listing the fleet no longer installs fails until
/// it is removed. A floor would let a unit leave the scan silently, and all
/// five fences over this domain can pass by seeing nothing.
///
/// Each fence keeps its own reach pin besides — `shutdown_exit_status`'s
/// `FLEET_SERVICES`, `install_notifies_restart`'s `REPLACING_ROLES`,
/// `unit_ownership`'s two-way match against the Metas. Those pin what survives
/// each fence's *filter* of this domain, which is the half this list cannot
/// see: a filter that stops admitting drop-ins narrows a fence without moving
/// anything here.
pub const FLEET_UNIT_FILES: &[&str] = &[
    "actual/actual.service",
    "baikal/baikal-birthday-sync.service",
    "baikal/baikal-birthday-sync.timer",
    "baikal/baikal-busy-sync.service",
    "baikal/baikal-busy-sync.timer",
    "bichon/bichon-archive.service",
    "bichon/bichon-archive.timer",
    "bichon/bichon-uidvalidity-watch.service",
    "bichon/bichon-uidvalidity-watch.timer",
    "bichon/bichon.service",
    "blocky/blocky.service",
    "blocky/lego-renew.service",
    "blocky/lego-renew.timer",
    "caddy/caddy.service",
    "caddy/caddy.service.d/caddy-env.conf",
    "calibre/calibre.service",
    "claude_code_remote/vibecoder.service",
    "cockpit/cockpit.socket.d/override.conf",
    "colporteur/colporteur.service",
    "colporteur/colporteur.timer",
    "freshrss/freshrss-update.service",
    "freshrss/freshrss-update.timer",
    "freshrss/freshrss.service",
    "gokapi/gokapi.service",
    "grimmory/grimmory.service",
    "headscale/headscale.service",
    "hermes/hermes-gateway.service (user)",
    "immich/immich-backup.service",
    "immich/immich-backup.timer",
    "immich/immich.service",
    "navidrome/navidrome.service.d/memory.conf",
    "navidrome/navidrome.service.d/start-limit.conf",
    "paperless/paperless-consumer.service",
    "paperless/paperless-scheduler.service",
    "paperless/paperless-task-queue.service",
    "paperless/paperless-webserver.service",
    "radio/icecast2.service.d/memory.conf",
    "radio/liquidsoap.service",
    "tgtg/tgtg.service",
];

fn pin_reach(scanned: &[InstalledUnit]) {
    // Before the set comparison, because a set cannot see this. Two tasks in
    // one role writing the same dest -- the `when: production` /
    // `when: not production` pair the repo writes elsewhere -- collapse into
    // one entry here and pass the pin. Downstream they do not collapse:
    // `start_limit` groups by `(role, name)` and sorts by `dropin`, which is
    // `None` for both, so the second file's directives override the first's as
    // if it were a drop-in, and `assignments_of` reports the same file name for
    // each. Nothing in the fleet writes one twice today.
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for id in scanned.iter().map(InstalledUnit::file_id) {
        *counted.entry(id).or_default() += 1;
    }
    let twice: Vec<&String> = counted
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, _)| id)
        .collect();
    assert!(
        twice.is_empty(),
        "these unit files are written by more than one task: {twice:?}. Which \
         one lands is a `when:` this scan does not weigh, and a fence reading \
         the merged directives would read the second as refining the first"
    );

    let seen: BTreeSet<String> = scanned.iter().map(InstalledUnit::file_id).collect();
    let listed: BTreeSet<String> = FLEET_UNIT_FILES.iter().map(|id| id.to_string()).collect();

    let unlisted: Vec<&String> = seen.difference(&listed).collect();
    let missing: Vec<&String> = listed.difference(&seen).collect();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "the fleet's unit files moved.\n  new, add to FLEET_UNIT_FILES in tests/common/units.rs: {unlisted:?}\n  gone, drop from FLEET_UNIT_FILES: {missing:?}"
    );
}

/// Every unit file and drop-in the fleet installs, sorted, with its reach
/// pinned against [`FLEET_UNIT_FILES`].
///
/// The pin runs on every call rather than living in a fence of its own, so a
/// scan that stopped reaching somewhere fails inside whichever fence relies on
/// it — the caller inherits the reach instead of trusting it.
pub fn fleet_units() -> Vec<InstalledUnit> {
    let mut units: Vec<InstalledUnit> = all_roles()
        .iter()
        .flat_map(|role| installed_by(role, &defaults(role)))
        .collect();
    units.sort_by(|a, b| {
        (&a.role, &a.name, a.scope, &a.dropin).cmp(&(&b.role, &b.name, b.scope, &b.dropin))
    });
    pin_reach(&units);
    units
}
