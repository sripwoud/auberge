//! Fleet-wide fence on Unit Ownership: which systemd units each App answers
//! for, declared as `units:` in its Playbook Meta.
//!
//! A failed deploy reads that declaration to report the state it left the
//! App's units in (#644) — a fact the CLI could not previously reach: a
//! Backup Recipe's `systemd_services` is a quiesce order 11 Apps have, and
//! `memory:` keys are opt-in per budget. Neither is an inventory.
//!
//! The declaration is hand-written, so it is fenced the way ADR-0028, 0035,
//! 0038 and 0040 fence theirs: everything the repo's own tasks reveal —
//! every unit file a role templates or copies, and every unit a role drops
//! in over, since a drop-in names the unit it refines — is computed here and
//! must be declared, in both directions. A unit a role installs without
//! either (a packaged template unit it only enables) cannot be computed off
//! any file, so it is declared with the reason the scan cannot see it.
//!
//! Deliberately outside the domain: units an App merely starts or depends on
//! (postgresql, redis, mariadb, docker, tailscaled) — they are shared
//! substrate with their own owners, not the App — and php-fpm, whose unit
//! name (`php8.4-fpm`) is a play-time package fact the roles themselves
//! discover from `package_facts`, so a Meta declaration of it would drift on
//! every PHP transition.

use std::collections::{BTreeMap, BTreeSet};

mod common;

use common::apps::{OwnedUnit, app_of, declared_units};
use common::units::{Scope, fleet_units};

/// A unit an App declares that no role installs a file for, and why the scan
/// cannot compute it. Each entry is checked to stay underivable: the day a
/// role starts templating it, the entry must go.
const DECLARED_WITHOUT_FILE: &[(&str, &str, &str)] = &[(
    "syncthing",
    "syncthing@{admin_user}.service",
    "a packaged template unit the role only enables per user; there is no \
     file to install, so no task reveals it",
)];

/// Every unit the fleet's own tasks reveal, keyed by the App that must
/// declare it.
///
/// The widest slice of the shared scan: every unit type, both managers, and
/// drop-ins included, since a drop-in names the unit it refines just as well as
/// a unit file does. The unit file and its drop-ins collapse to one name here —
/// this fence asks who owns the unit, not what any one file says about it.
fn computed_units() -> BTreeSet<OwnedUnit> {
    let mut by_role: BTreeMap<String, BTreeSet<(String, Scope)>> = BTreeMap::new();
    for unit in fleet_units() {
        by_role
            .entry(unit.role.clone())
            .or_default()
            .insert((unit.name, unit.scope));
    }
    let mut out = BTreeSet::new();
    for (role, installed) in by_role {
        // A role that installs units and maps to no Meta is a hard stop: its
        // units would have nowhere to be declared.
        let app = app_of(&role).unwrap_or_else(|| {
            panic!(
                "{role} installs systemd units but maps to no Playbook Meta; \
                 create `<app>.meta.yml` so the units have somewhere to be \
                 declared"
            )
        });
        for (unit, scope) in installed {
            out.insert(OwnedUnit {
                app: app.clone(),
                unit,
                scope,
            });
        }
    }
    out
}

fn ids(units: &BTreeSet<OwnedUnit>) -> BTreeSet<String> {
    units.iter().map(OwnedUnit::id).collect()
}

/// Computed -> declared. A role that installs or drops in over a unit has
/// revealed it; the App must own up to it, or a failed deploy of exactly
/// that App reads out nothing — the silence #644 exists to end.
#[test]
fn test_every_unit_a_roles_tasks_reveal_is_declared_by_its_app() {
    let computed = ids(&computed_units());
    let declared = ids(&declared_units());
    assert_eq!(
        computed.difference(&declared).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "roles reveal units their Apps do not declare; add them to the App's \
         `units:` in its Playbook Meta"
    );
}

/// Declared -> computed, with the underivable remainder named. A declaration
/// no task backs is either a packaged unit with its reason listed in
/// DECLARED_WITHOUT_FILE, or a claim about the fleet nobody checks.
#[test]
fn test_every_declared_unit_is_revealed_by_a_task_or_names_why_not() {
    let computed = ids(&computed_units());
    let declared = ids(&declared_units());
    let excused: BTreeSet<String> = DECLARED_WITHOUT_FILE
        .iter()
        .map(|(app, unit, _)| format!("{app}/{unit}"))
        .collect();

    let surplus: BTreeSet<String> = declared.difference(&computed).cloned().collect();
    assert_eq!(
        surplus.difference(&excused).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "Metas declare units no role's tasks reveal and no DECLARED_WITHOUT_FILE \
         entry excuses; either the unit is gone or the scan stopped seeing it — \
         the second is the dangerous one"
    );
    assert_eq!(
        excused.difference(&surplus).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "DECLARED_WITHOUT_FILE excuses units that are either no longer declared \
         or now revealed by a task; drop the stale entry"
    );
}

/// The one ownership fact the scan cannot check per unit: an App that
/// installs no units at all declares none, so a `units:` key on it would be
/// an inventory of nothing. Pinned so the boundary stays deliberate: yourls
/// runs on php-fpm (a play-time package fact) and mariadb (shared
/// substrate), neither of which is this fence's domain.
#[test]
fn test_apps_outside_the_domain_declare_no_units() {
    let declared = declared_units();
    for app in ["yourls", "apps", "infrastructure", "hardening"] {
        assert!(
            !declared.iter().any(|owned| owned.app == app),
            "{app} declares units; its exclusion from the ownership domain was \
             deliberate — revisit the doc comment at the top of this file \
             before changing it"
        );
    }
}
