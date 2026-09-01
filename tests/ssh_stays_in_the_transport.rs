//! #669: the `SshSession` trait is the only way to reach a Host, and exactly
//! one module spells the ssh and scp command lines that get there.
//!
//! Rust already enforces the half that is about a *type*: `SshTransport` lives
//! in a private `mod transport`, so nothing outside `services::ssh` can name
//! it. What Rust cannot see is a module that skips the type and spawns `ssh`
//! itself — which is exactly how the repo accumulated three hand-rolled
//! reachability probes with three different timeout, multiplexing and error
//! behaviours, plus two more raw sites. That is what this fences.
//!
//! Three assertions, because a confinement rule can fail in three directions:
//! something escapes; the confining module stops confining anything, leaving
//! the scan to pass over an empty domain; or a declared exception silently
//! stops being exceptional. The fourth — a walk that stops reaching the
//! crate's modules — is pinned inside `crate_source::modules` and inherited
//! here (#679).

mod crate_source;

use crate_source::{modules, names_in_code};

/// The one module allowed to spawn ssh or scp, and to spell their options.
const TRANSPORT: &str = "src/services/ssh/transport.rs";

/// Spawning `ssh` or `scp` directly. `rsync` is deliberately absent: three
/// modules run it legitimately, and what matters for rsync is where its `-e`
/// argument comes from, which [`DECLARED_HAND_BUILT_TRANSPORT`] covers.
const SPAWNS: &[&str] = &["Command::new(\"ssh\")", "Command::new(\"scp\")"];

/// ssh client options. A module spelling one of these is deciding how the
/// connection behaves, which is the transport's job — and the drift these
/// caught in the first place was three probes disagreeing about exactly
/// `ConnectTimeout` and the `Control*` trio.
const OPTIONS: &[&str] = &[
    "ControlMaster",
    "ControlPath",
    "ControlPersist",
    "ConnectTimeout",
    "BatchMode",
];

/// The declared exception (#669): music sync builds its own `ssh` string for
/// rsync's `-e`, because that transfer carries a hand-picked flag set and
/// parses live progress off `--info=progress2`.
///
/// Listed with the marker its site must carry, so the exception cannot decay
/// into an undocumented one — a reader who finds the hand-built string has
/// something to search for.
const DECLARED_HAND_BUILT_TRANSPORT: (&str, &str) = ("src/commands/sync.rs", "#669");

#[test]
fn only_the_transport_spawns_ssh_or_scp() {
    let mut offenders: Vec<String> = Vec::new();
    for module in modules() {
        if module.repo_relative == TRANSPORT {
            continue;
        }
        for spawn in SPAWNS {
            if names_in_code(&module.source, spawn) {
                offenders.push(format!(
                    "  {} runs `{spawn}` — reach the Host through the SshSession \
                     trait instead (#669)",
                    module.repo_relative
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "ssh escaped the transport:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn only_the_transport_spells_ssh_options() {
    let mut offenders: Vec<String> = Vec::new();
    for module in modules() {
        if module.repo_relative == TRANSPORT {
            continue;
        }
        for option in OPTIONS {
            if names_in_code(&module.source, option) {
                offenders.push(format!(
                    "  {} spells `{option}` — how a connection behaves is the \
                     transport's to decide (#669)",
                    module.repo_relative
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "ssh options escaped the transport:\n{}",
        offenders.join("\n")
    );
}

/// Without this, deleting the transport — or emptying it out into some other
/// module — would leave both scans above passing over a domain that no longer
/// holds the thing they fence.
///
/// The module is looked up in the walk rather than read from disk beside it, so
/// a transport the walk cannot reach fails here too.
#[test]
fn the_transport_still_holds_the_command_lines() {
    let walked = modules();
    let transport = walked
        .iter()
        .find(|module| module.repo_relative == TRANSPORT)
        .unwrap_or_else(|| panic!("{TRANSPORT} must exist to confine ssh"));

    for spawn in SPAWNS {
        assert!(
            names_in_code(&transport.source, spawn),
            "{TRANSPORT} no longer runs `{spawn}` — either it moved (update \
             TRANSPORT) or the CLI stopped shelling out (drop the row)"
        );
    }
    for option in OPTIONS {
        assert!(
            names_in_code(&transport.source, option),
            "{TRANSPORT} no longer spells `{option}` — either it moved (update \
             TRANSPORT) or the option is gone (drop the row)"
        );
    }
}

/// The declared exception has to stay declared. A site that keeps building its
/// own ssh string but loses the marker explaining why is indistinguishable from
/// drift, and this is the assertion that makes the comment load-bearing rather
/// than decorative.
#[test]
fn the_hand_built_transport_still_declares_itself() {
    let (site, marker) = DECLARED_HAND_BUILT_TRANSPORT;
    let walked = modules();
    let module = walked
        .iter()
        .find(|module| module.repo_relative == site)
        .unwrap_or_else(|| panic!("{site} must exist to carry the exception"));

    let builds_transport = module.source.contains("\"ssh -p ");
    assert!(
        builds_transport,
        "{site} no longer builds its own ssh string — drop it from \
         DECLARED_HAND_BUILT_TRANSPORT so the exception stops being claimed"
    );
    assert!(
        module.source.contains(marker),
        "{site} builds its own ssh string but no longer cites {marker} — an \
         undeclared exception is indistinguishable from drift"
    );
}
