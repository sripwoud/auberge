//! #787's routing policy: `prefer_tailnet` decides a Host's address, and
//! exactly one function is allowed to know that.
//!
//! A sibling to `one_route_to_a_host.rs` (#784) and
//! `the_include_follows_the_roster.rs` (#786) rather than an extension of
//! either — widening a fence's subject is how a scan starts passing
//! vacuously, and each of the three asks a different question. That one
//! fences where an address comes *from*; this one fences who is allowed to
//! *choose*, and which of the two answers each consumer takes.
//!
//! Four directions, because the policy fails in four:
//!
//! 1. a second module reimplements the choice from `prefer_tailnet`;
//! 2. the generated ssh include picks up a per-invocation `--via`, making
//!    interactive ssh permanently disagree with the roster — the #780
//!    divergence, reintroduced by the flag meant to work around it;
//! 3. the override is installed or checked from somewhere other than `main`,
//!    so a command can silently change the route mid-run;
//! 4. the Inventory's two addresses collapse back into one name, which is how
//!    `dns set-all` would publish a CGNAT address as a public A record.
//!
//! Each direction is paired with a non-vacuity claim, so deleting the thing
//! being fenced fails here rather than leaving an empty domain scanning clean.

mod crate_source;

use crate_source::{Module, find, modules, names_in_code};

/// The only modules allowed to *read* `prefer_tailnet`.
///
/// Scanned as the field access `.prefer_tailnet`, not the bare name: test
/// fixtures across four modules initialise the field in a `Host` literal, and
/// `main.rs`'s `--via` help text names it in prose. Cutting `#[cfg(test)]`
/// instead would need the trailing-test-module care ADR-0070 documents, and
/// `services/ssh.rs` carries a `#[cfg(test)]` fixture *outside* its test
/// module — the exact shape that made that cut silently drop the domain.
/// The limit the narrower needle accepts: a module that destructured a `Host`
/// to reach the field would evade this. Nothing in the crate destructures one,
/// and `route::resolve` is the shorter path to the same answer.
///
/// - `src/services/route.rs` turns it into an address. That is the policy.
/// - `src/hosts.rs` declares, (de)serializes and validates the field —
///   declaration-level work, exempt for the same reason it is exempt from
///   `one_route_to_a_host.rs`'s address scan.
/// - `src/commands/host.rs` is the operator's surface: the `host edit` prompt
///   that sets it and the `host list` column that shows it. Neither decides
///   an address.
///
/// Anything else naming the field is deciding a route off the declaration,
/// which is the six-read-sites problem #784 removed, re-entering through the
/// field #787 added.
const POLICY_READERS: &[&str] = &[
    "src/commands/host.rs",
    "src/hosts.rs",
    "src/services/route.rs",
];

/// The resolver: the one module that may turn the policy into an address, and
/// the only one that may read the `--via` override.
const RESOLVER: &str = "src/services/route.rs";

/// The generated ssh include. It must read the *declared* route and never the
/// resolved one: the file outlives the command that wrote it, and ADR-0070
/// regenerates it on every roster write — `--via` included.
const SSH_INCLUDE: &str = "src/services/ssh_include.rs";

/// Where the run's `--via` is installed and where its effect is checked. Both
/// belong to the binary's entry point: a command that could call
/// [`set_override`](auberge::services::route::set_override) could move the
/// route out from under a Host it has already resolved, and one that could
/// call `ensure_override_reached_a_host` could report the flag as applied
/// without applying it.
const OVERRIDE_OWNER: &str = "src/main.rs";
const OVERRIDE_ENTRY_POINTS: &[&str] = &["set_override(", "ensure_override_reached_a_host("];

/// The Inventory names its two addresses apart (#787): `public_address` is
/// what DNS publishes, `connect_address` is where the CLI connects. The bare
/// inventory-variable spelling may survive only where it is genuinely the
/// ansible variable — the `#[serde(rename)]` that reads `inventory.yml`, and
/// the key `ansible_runner` writes into a run's generated inventory.
const ANSIBLE_HOST_SITES: &[&str] = &[
    "src/services/inventory.rs",
    "src/services/ansible_runner.rs",
];

fn other_modules<'a>(walked: &'a [Module], allowed: &[&str]) -> Vec<&'a Module> {
    walked
        .iter()
        .filter(|module| !allowed.contains(&module.repo_relative.as_str()))
        .collect()
}

#[test]
fn only_the_resolver_decides_a_route_from_prefer_tailnet() {
    let walked = modules();
    let offenders: Vec<&str> = other_modules(&walked, POLICY_READERS)
        .into_iter()
        .filter(|module| names_in_code(&module.source, ".prefer_tailnet"))
        .map(|module| module.repo_relative.as_str())
        .collect();

    assert!(
        offenders.is_empty(),
        "prefer_tailnet escaped the resolver — take a Route from \
         services::route::resolve instead of deciding one (#787): {offenders:?}"
    );
}

/// Without this, emptying the resolver out would leave the scan above passing
/// over a policy nothing implements.
#[test]
fn the_resolver_still_decides_from_prefer_tailnet() {
    let walked = modules();
    assert!(
        names_in_code(&find(&walked, RESOLVER).source, ".prefer_tailnet"),
        "{RESOLVER} no longer reads prefer_tailnet — either it moved (update \
         RESOLVER) or the policy stopped existing, which leaves the scan \
         above vacuous"
    );
}

/// The include is written from the roster's decision, never from this
/// invocation's. `auberge --via public host edit x` regenerates the include
/// (ADR-0070 binds regeneration to every roster write); if that regeneration
/// saw the override, every alias would be republished on the public address
/// for good, and interactive `ssh x` would take a route nobody declared.
#[test]
fn the_ssh_include_is_written_from_the_declared_route() {
    let walked = modules();
    let include = find(&walked, SSH_INCLUDE);

    assert!(
        names_in_code(&include.source, "route::declared("),
        "{SSH_INCLUDE} no longer resolves a declared Route — the include's \
         connection directives must come off one (ADR-0070)"
    );
    assert!(
        !names_in_code(&include.source, "route::resolve("),
        "{SSH_INCLUDE} resolves a Route with the --via override applied; a \
         per-invocation flag must not be published to a file that outlives \
         the command (#787)"
    );
}

/// And the two are genuinely different functions, so the assertion above is
/// not two names for one behaviour.
#[test]
fn the_resolver_offers_both_a_declared_and_an_overridable_route() {
    let walked = modules();
    let resolver = &find(&walked, RESOLVER).source;

    for entry in ["pub fn declared(", "pub fn resolve("] {
        assert!(
            names_in_code(resolver, entry),
            "{RESOLVER} no longer defines `{entry}` — the include fence above \
             cannot tell the two routes apart without both"
        );
    }
}

#[test]
fn nothing_but_main_installs_or_checks_the_via_override() {
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();

    for module in other_modules(&walked, &[OVERRIDE_OWNER, RESOLVER]) {
        for entry in OVERRIDE_ENTRY_POINTS {
            if names_in_code(&module.source, entry) {
                offenders.push(format!("  {} names {entry}", module.repo_relative));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the --via override is installed or checked outside the binary's \
         entry point (#787):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn main_still_installs_and_checks_the_via_override() {
    let walked = modules();
    let main = &find(&walked, OVERRIDE_OWNER).source;

    for entry in OVERRIDE_ENTRY_POINTS {
        assert!(
            names_in_code(main, entry),
            "{OVERRIDE_OWNER} no longer calls {entry} — a --via that is never \
             installed silently does nothing, and one that is never checked \
             silently does nothing on a command that routes nowhere"
        );
    }
}

/// `ansible_host` answered two questions until #787 made them diverge, and
/// the reader who took the wrong one was `sync music`. Each is now named for
/// its answer; a module reintroducing the ambiguous spelling is a module
/// about to pick the wrong address.
#[test]
fn the_inventorys_two_addresses_stay_named_apart() {
    let walked = modules();
    let offenders: Vec<&str> = other_modules(&walked, ANSIBLE_HOST_SITES)
        .into_iter()
        .filter(|module| names_in_code(&module.source, "ansible_host"))
        .map(|module| module.repo_relative.as_str())
        .collect();

    assert!(
        offenders.is_empty(),
        "`ansible_host` names neither address since #787 — read \
         `vars.public_address` for what DNS publishes, `connect_address` for \
         where the CLI connects: {offenders:?}"
    );
}

/// Both spellings have to still exist, or the scan above passes over an
/// Inventory that has stopped telling the two addresses apart at all.
#[test]
fn the_inventory_still_carries_both_addresses() {
    let walked = modules();
    let inventory = &find(&walked, "src/services/inventory.rs").source;

    for field in ["pub public_address:", "pub connect_address:"] {
        assert!(
            names_in_code(inventory, field),
            "services/inventory.rs no longer declares `{field}` — the two \
             addresses have collapsed back into one (#787)"
        );
    }
}
