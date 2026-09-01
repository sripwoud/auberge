//! #780's Route seam (#784): every ssh, scp, rsync and ansible connection the
//! CLI makes collapses through `services::route::resolve`, so a future
//! policy (#787's `prefer_tailnet`) has one seam to change rather than six
//! read sites of `Host::address`.
//!
//! A sibling to `ssh_stays_in_the_transport.rs`, inheriting the module walk
//! from `crate_source` (#679) — that one fences command *spelling*, this
//! fences address *provenance*, and widening its subject instead of adding a
//! sibling is how a scan starts passing vacuously. Three assertions, because
//! a seam like this fails in three directions: something bypasses it; the
//! modules it binds stop existing, leaving the scan to pass over an empty
//! domain; or the declared exception silently stops being exceptional.

mod crate_source;

use crate_source::modules;

/// The six modules #780 named as answering "how do we reach this host" off
/// `Host::address` directly, before this seam existed.
///
/// `hosts.rs` is deliberately not among them: it legitimately reads the field
/// for serialization, so listing it here would have the fence exempt exactly
/// the module it most needs to constrain. Nor are `commands/host.rs` and
/// `commands/backup.rs` — the handful of remaining reads there are
/// declaration display and an edit prompt's default, not a connection
/// decision, and must keep showing the Host's own declared address forever,
/// including after #787 adds a routing policy that could otherwise leak into
/// them.
const CONSUMERS: &[&str] = &[
    "src/services/ssh.rs",
    "src/services/ssh/transport.rs",
    "src/services/ssh_include.rs",
    "src/services/ansible_runner.rs",
    "src/services/inventory.rs",
    "src/commands/sync.rs",
];

/// The one module allowed to read `Host::address` in order to decide how to
/// reach a Host.
const RESOLVER: &str = "src/services/route.rs";

/// The declared exception (#780): a virgin host has no tailnet yet, and no
/// verified `hosts.toml` identity of its own — the operator just confirmed an
/// IP at a prompt — so bootstrap builds its `Route` directly rather than
/// resolving one from a `Host` that does not exist yet in any trustworthy
/// sense.
///
/// Listed with the marker its site must carry, so the exception cannot decay
/// into an undocumented one — mirrors `DECLARED_HAND_BUILT_TRANSPORT` in
/// `ssh_stays_in_the_transport.rs`.
const DECLARED_BOOTSTRAP_EXCEPTION: (&str, &str) = ("src/commands/ansible.rs", "#780");

/// Modules that build an `InventoryHost` from `services::inventory::Host`
/// (the ansible/inventory.yml-shaped type) rather than `crate::hosts::Host`,
/// and so cannot reach `services::route::resolve` — they go through
/// `services::inventory::Host::route` instead, or, the one declared
/// exception, build a `Route` by hand.
const ROUTE_LITERAL_CALLERS: &[&str] = &["src/commands/ansible.rs", "src/commands/deploy.rs"];

/// How many lines above a `Route {` literal a marker comment may sit and
/// still count as documenting *that* literal, rather than some earlier,
/// unrelated one elsewhere in a long function.
const MARKER_WINDOW: usize = 12;

/// `true` when one of the `window` lines up to and including `literal_line`
/// contains `marker`.
fn marker_precedes(source: &str, literal_line: usize, marker: &str, window: usize) -> bool {
    let lines: Vec<&str> = source.lines().collect();
    let start = literal_line.saturating_sub(window);
    lines[start..=literal_line]
        .iter()
        .any(|line| line.contains(marker))
}

/// `true` when `source` contains `needle` outside of a `//` comment line.
///
/// Comments are excluded on purpose, exactly as in the sibling fence: this
/// module's own doc comment, and `hosts.rs`'s, both discuss `Host::address`
/// in prose, and a rule that fires on its own explanation teaches people to
/// delete the explanation rather than fix the code.
fn names_in_code(source: &str, needle: &str) -> bool {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .any(|line| line.contains(needle))
}

fn find<'a>(walked: &'a [crate_source::Module], path: &str) -> &'a crate_source::Module {
    walked
        .iter()
        .find(|module| module.repo_relative == path)
        .unwrap_or_else(|| panic!("{path} must exist to be checked"))
}

#[test]
fn only_the_resolver_reads_a_hosts_address() {
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();
    for path in CONSUMERS {
        let module = find(&walked, path);
        if names_in_code(&module.source, "host.address") {
            offenders.push(format!(
                "  {path} reads a Host's address directly — reach it through \
                 services::route::resolve instead (#780)"
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "Host::address escaped the resolver:\n{}",
        offenders.join("\n")
    );
}

/// Without this, deleting the resolver — or emptying it out so it no longer
/// reads anything — would leave the scan above passing over a domain that no
/// longer holds the thing it fences.
#[test]
fn the_resolver_still_reads_a_hosts_address() {
    let walked = modules();
    let resolver = find(&walked, RESOLVER);

    assert!(
        names_in_code(&resolver.source, "host.address"),
        "{RESOLVER} no longer reads a Host's address — either it moved \
         (update RESOLVER) or the seam stopped resolving anything, which \
         would leave the scan above vacuous"
    );
}

/// Every raw `Route { .. }` literal in a module that cannot call
/// `services::route::resolve` must be the one declared exception, marked at
/// that exact literal — not merely present somewhere in the same file.
///
/// Line-anchored rather than a whole-file substring check: `commands/
/// ansible.rs` legitimately builds an `InventoryHost` at several call sites,
/// so a marker anywhere in the file could not tell "bootstrap's own literal
/// is documented" apart from "this file happens to contain both strings for
/// unrelated reasons" — an undeclared literal added metres away from the real
/// exception would otherwise read as covered.
#[test]
fn only_the_bootstrap_exception_builds_its_own_route() {
    let (exception_site, marker) = DECLARED_BOOTSTRAP_EXCEPTION;
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();

    for path in ROUTE_LITERAL_CALLERS {
        let module = find(&walked, path);
        for (index, line) in module.source.lines().enumerate() {
            if line.trim_start().starts_with("//") || !line.contains("route::Route {") {
                continue;
            }
            let declared = *path == exception_site
                && marker_precedes(&module.source, index, marker, MARKER_WINDOW);
            if !declared {
                offenders.push(format!(
                    "  {path}:{} builds a Route literal directly — call \
                     Host::route (or services::route::resolve) instead, or \
                     mark it as the declared bootstrap exception ({marker})",
                    index + 1
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a Route literal escaped the seam:\n{}",
        offenders.join("\n")
    );
}

/// The declared exception has to stay declared, and it has to stay real: a
/// site that keeps building its own `Route` but loses the marker explaining
/// why is indistinguishable from drift, and a site that stopped needing the
/// exception at all is a stale claim on the seam's one carve-out.
#[test]
fn the_bootstrap_exception_still_builds_its_own_route() {
    let (site, marker) = DECLARED_BOOTSTRAP_EXCEPTION;
    let walked = modules();
    let module = find(&walked, site);

    let builds_its_own_route = names_in_code(&module.source, "route::Route {");
    assert!(
        builds_its_own_route,
        "{site} no longer builds a Route literal directly — drop it from \
         DECLARED_BOOTSTRAP_EXCEPTION so the exception stops being claimed"
    );
    assert!(
        module.source.contains(marker),
        "{site} builds its own Route but no longer cites {marker} — an \
         undeclared exception is indistinguishable from drift"
    );
}
