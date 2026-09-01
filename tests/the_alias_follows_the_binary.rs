//! #800: carrying an already-verified host key onto its `HostKeyAlias` is
//! bound to the roster *read*, because the thing that needs migrating is the
//! binary and not the roster.
//!
//! The defect this closes: #785 made every connection send
//! `-o HostKeyAlias=<name>`, and #786 bound the migration that creates those
//! `known_hosts` entries to `HostManager::save_hosts`. Upgrading is not a
//! roster mutation, so the first run of a binary carrying #785 found no entry
//! under the alias and failed `Host key verification failed` on every command
//! — `auberge-backup.service` at 03:00 included — until the operator happened
//! to run a `host` subcommand, a recovery the error text names nowhere.
//!
//! Two clocks, one of which nobody winds. This fence holds the trigger and the
//! senders on the clock that ticks: reading the roster, which every one of
//! them does first.
//!
//! A sibling to `the_include_follows_the_roster.rs`, inheriting the module
//! walk from `crate_source` (#679): that one fences whether the published
//! route was rewritten, this fences whether the identity it is checked under
//! is trusted. Both are #780's split showing up in a different file, and
//! widening either one's subject is how a scan starts passing vacuously
//! (ADR-0067 makes the same call).
//!
//! What is asserted textually is the trigger and the set of senders. That
//! every sender is *downstream* of the read is a fact about the crate rather
//! than a scan: `HostsConfig` is private to `src/hosts.rs`, so a `Host` can
//! only come from `HostManager::load_hosts`, and the compiler answers it.
//! The behavioural half — that reading actually migrates — is asserted where
//! it can be observed, in
//! `hosts::tests::read_roster_migrates_every_hosts_known_hosts_alias`.

mod crate_source;

use crate_source::{find, modules, names_in_code};

/// Every module that puts `HostKeyAlias` on the wire. Each one connects to an
/// address and is checked against the Host's name, so each one is unusable
/// until the migration has run.
const ALIAS_SENDERS: &[&str] = &[
    "src/services/ssh/transport.rs",
    "src/services/ansible_runner.rs",
    "src/commands/sync.rs",
];

/// The module that writes `HostKeyAlias` into `~/.ssh/config.d/auberge.conf`
/// rather than onto a connection of its own. It is not a sender: the stanza is
/// what interactive `ssh <name>` obeys, and the CLI never matches it.
const ALIAS_PUBLISHER: &str = "src/services/ssh_include.rs";

const ALIAS_OPTION: &str = "HostKeyAlias";

/// The module holding the roster, and the one function in it whose every
/// caller — `get_host`, `list_hosts_filtered`, and every mutation path —
/// passes through.
const ROSTER_READER: &str = "src/hosts.rs";
const ROSTER_READ_FN: &str = "fn read_roster(";

/// Where the migration is defined. Naming it there is not calling it.
const MIGRATION_MODULE: &str = "src/services/known_hosts.rs";
const MIGRATION_CALL: &str = "known_hosts::migrate_roster(";

/// The fix #780 settled against, and the one a maintainer meeting
/// `Host key verification failed` will reach for first.
const TOFU_OPTION: &str = "StrictHostKeyChecking";

/// The body of the `fn` whose declaration contains `decl`, from the
/// declaration to the closing brace at the declaration's own indentation.
///
/// The indentation is read off the declaration rather than assumed to be
/// column zero. `read_roster` is a method, so its brace is `    }` and a
/// column-zero search runs to the end of `impl HostManager` — which would
/// make the assertion below pass with the migration call sitting in
/// `save_hosts`, the exact binding #800 moves it out of. The sibling helper in
/// `the_include_follows_the_roster.rs` reads a free function and does not have
/// the problem; this one is not shared with it precisely so each states the
/// shape it reads.
fn fn_body<'a>(source: &'a str, decl: &str) -> &'a str {
    let start = source
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} must exist to be checked"));
    let line_start = source[..start].rfind('\n').map_or(0, |at| at + 1);
    let close = format!("\n{}}}\n", &source[line_start..start]);

    let rest = &source[start..];
    let end = rest
        .find(&close)
        .unwrap_or_else(|| panic!("{decl} must have a closing brace at its own indentation"));
    &rest[..end]
}

/// The module's source with its trailing `#[cfg(test)] mod tests` cut off.
///
/// Every sender asserts its own argv in its tests, and `hosts.rs` asserts the
/// generated stanza in one of its own — counting those would make the closed
/// set below fire on the tests that keep it honest, and would make
/// `hosts.rs` read as a sender it is not.
///
/// Cut on the test *module* rather than a bare `#[cfg(test)]`, and stated
/// here rather than shared with `the_include_follows_the_roster.rs`: the two
/// spellings differ by hundreds of lines in `hosts.rs`, and a fence that must
/// not read test code says where it cut (`crate_source`). If the anchor stops
/// matching, the whole file is scanned, which fails loudly rather than
/// quietly.
fn without_tests(source: &str) -> &str {
    match source.find(TEST_MODULE) {
        Some(at) => &source[..at],
        None => source,
    }
}

const TEST_MODULE: &str = "#[cfg(test)]\nmod tests {";

/// A module that starts sending the alias is a module that starts depending on
/// the migration having run. Adding one is a decision to be made with this
/// fence in view, not a line of argv.
#[test]
fn only_declared_sites_send_the_alias() {
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();

    for module in &walked {
        let declared = ALIAS_SENDERS.contains(&module.repo_relative.as_str())
            || module.repo_relative == ALIAS_PUBLISHER;
        if declared || !names_in_code(without_tests(&module.source), ALIAS_OPTION) {
            continue;
        }
        offenders.push(format!(
            "  {} sends {ALIAS_OPTION} — every alias-sending path depends on \
             the migration in {ROSTER_READER}'s `{ROSTER_READ_FN}` having run, \
             so add it to ALIAS_SENDERS deliberately (#800)",
            module.repo_relative
        ));
    }

    assert!(
        offenders.is_empty(),
        "an undeclared module sends the host key alias:\n{}",
        offenders.join("\n")
    );
}

/// Without this, deleting `-o HostKeyAlias` from every connection would leave
/// the scan above passing over a crate where the migration protects nothing —
/// and would silently put host identity back on the address, which is the
/// split #780 exists to close.
#[test]
fn every_declared_site_still_sends_the_alias() {
    let walked = modules();

    for path in ALIAS_SENDERS.iter().chain([ALIAS_PUBLISHER].iter()) {
        let module = find(&walked, path);
        assert!(
            names_in_code(without_tests(&module.source), ALIAS_OPTION),
            "{path} no longer sends {ALIAS_OPTION} — either it moved (update \
             ALIAS_SENDERS) or host identity went back to being a function of \
             the address (#780), which would leave the scan above vacuous"
        );
    }
}

/// The trigger, on the event that tracks the binary. Mutation-tested by
/// deleting the call: this fails, and so does
/// `hosts::tests::read_roster_migrates_every_hosts_known_hosts_alias`.
#[test]
fn the_roster_read_migrates_the_alias() {
    let walked = modules();
    let reader = find(&walked, ROSTER_READER);
    let body = fn_body(without_tests(&reader.source), ROSTER_READ_FN);

    assert!(
        names_in_code(body, MIGRATION_CALL),
        "{ROSTER_READER}'s `{ROSTER_READ_FN}` no longer calls \
         {MIGRATION_CALL} — an upgraded binary would send \
         `{ALIAS_OPTION}=<name>` with nothing under that name in \
         ~/.ssh/known_hosts and fail every command until the operator \
         happened to mutate the roster (#800)"
    );
}

/// And on that event alone. A command calling the migration itself is a
/// command remembering to — the design that left `detect-tailscale-ip`
/// without a regeneration (#786) and left an upgrade without a migration
/// (#800).
#[test]
fn only_the_roster_read_triggers_the_migration() {
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();

    for module in &walked {
        if module.repo_relative == ROSTER_READER || module.repo_relative == MIGRATION_MODULE {
            continue;
        }
        if names_in_code(&module.source, MIGRATION_CALL) {
            offenders.push(format!(
                "  {} calls {MIGRATION_CALL} — the migration is bound to the \
                 roster read in {ROSTER_READER}, so no command has to remember \
                 it (#800)",
                module.repo_relative
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "the known_hosts migration is triggered outside the roster read:\n{}",
        offenders.join("\n")
    );
}

/// The rejected alternative stays rejected. `StrictHostKeyChecking=accept-new`
/// makes `Host key verification failed` go away by trusting whatever answers
/// under an alias the CLI has never seen — inside a change whose whole purpose
/// is knowing where traffic goes (#780). It belongs only in the generated
/// include, where it governs interactive `ssh <name>` on a Host the operator
/// is adding by hand.
///
/// This is what keeps the migration load-bearing: without the ban, the next
/// regression could be "fixed" by opting into TOFU, and every assertion above
/// would still pass.
#[test]
fn no_alias_sender_accepts_a_new_key_under_the_alias() {
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();

    for path in ALIAS_SENDERS {
        let module = find(&walked, path);
        if names_in_code(without_tests(&module.source), TOFU_OPTION) {
            offenders.push(format!(
                "  {path} sets {TOFU_OPTION} — a connection sending \
                 {ALIAS_OPTION} must find the alias already trusted, not \
                 accept-new whatever answers under it (#780, #800)"
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "an alias sender opts into TOFU:\n{}",
        offenders.join("\n")
    );
}
