//! ADR-0047: a seam is a runtime argument, never a `cfg`.
//!
//! A `#[cfg(not(test))]` item makes the test binary and the shipped binary hold
//! different code, so the production branch is the one branch no test can
//! reach. The Backup Session had exactly one, and it cost the module every test
//! it looks like it has: `make_recipe_progress` returned a hidden
//! `TerminalProgress` under `cfg(test)`, so `MockProgress` — twenty call sites
//! away in the Recipe Executor's tests — could never observe a Session, and
//! `create`'s own output went unasserted through `eprintln!` (#670).
//!
//! The second half of the rule is where the seam is filled from. A runner that
//! builds its own `TerminalProgress` has an injected seam in name only: restic
//! push and prune each constructed one and so had no tests at all. Every
//! construction now lives in the command layer, which is the only layer allowed
//! to know there is a terminal.
//!
//! Both scans read `src/` through `crate_source::modules`, so their reach is
//! inherited rather than trusted: a walk that stopped seeing the tree would fail
//! there instead of passing here over an empty domain (#679, ADR-0046).

mod crate_source;

use crate_source::modules;

/// A `cfg` predicate that is false exactly when the test harness is compiled
/// in — the shape that forks production away from what tests see.
///
/// Matched as raw text because that is the question: whether the *source* holds
/// the fork. `cfg(all(not(test), unix))` and `cfg_attr(not(test), …)` are the
/// same fork spelled longer, and both contain this.
const PRODUCTION_ONLY_CFG: &str = "not(test)";

/// The lines of `source` that fork on the harness.
///
/// Comment lines are skipped so prose may name the shape it forbids — this
/// file's own module doc does, and an ADR reference in a doc comment is not a
/// fork. Block comments are not skipped; the crate uses none.
fn forking_lines(source: &str) -> Vec<(usize, &str)> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .filter(|(_, line)| line.contains(PRODUCTION_ONLY_CFG))
        .map(|(index, line)| (index + 1, line.trim()))
        .collect()
}

#[test]
fn no_module_forks_production_away_from_its_tests() {
    let mut offenders: Vec<String> = Vec::new();

    for module in modules() {
        for (line_number, line) in forking_lines(&module.source) {
            offenders.push(format!("  {}:{line_number}: {line}", module.repo_relative));
        }
    }

    assert!(
        offenders.is_empty(),
        "a compile-time seam forks production away from what the tests see \
         — inject the collaborator instead (ADR-0047):\n{}",
        offenders.join("\n")
    );
}

/// Without this the scan above passes by finding nothing whether or not it can
/// see anything, which is the failure mode `crate_source`'s reach pin exists
/// for one level up: reach proves the files were read, this proves the reading
/// would notice.
#[test]
fn the_scan_finds_the_fork_it_looks_for() {
    let forked = "#[cfg(not(test))]\nfn live() {}\n#[cfg(test)]\nfn faked() {}\n";
    assert_eq!(forking_lines(forked), vec![(1, "#[cfg(not(test))]")]);

    let longer = "#[cfg_attr(not(test), inline)]\n#[cfg(all(not(test), unix))]\n";
    assert_eq!(forking_lines(longer).len(), 2);
}

#[test]
fn the_scan_passes_over_test_only_items_and_prose() {
    let benign = concat!(
        "//! Replaced a `#[cfg(not(test))]` seam with an injected factory.\n",
        "/// See ADR-0047 on `cfg(not(test))`.\n",
        "#[cfg(test)]\n",
        "pub fn hidden() {}\n",
        "#[cfg(not(feature = \"tokio\"))]\n",
        "fn blocking() {}\n",
    );
    assert!(forking_lines(benign).is_empty());
}

/// The renderer only a command may build.
const TERMINAL_PROGRESS: &str = "TerminalProgress::";

/// Modules allowed to name it: every command, plus the renderer's own module,
/// whose tests construct the hidden variant.
fn may_build_terminal_progress(module: &str) -> bool {
    module.starts_with("src/commands/") || module == "src/services/progress.rs"
}

/// Lines of `source` that construct the terminal renderer.
///
/// Comment lines are skipped for the same reason as above — this file's own
/// doc names the type. `Progress` the trait and `MockProgress` are not matched:
/// the needle carries the concrete type's full name and its `::`.
fn constructing_lines(source: &str) -> Vec<(usize, &str)> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .filter(|(_, line)| line.contains(TERMINAL_PROGRESS))
        .map(|(index, line)| (index + 1, line.trim()))
        .collect()
}

#[test]
fn only_a_command_builds_the_terminal_progress() {
    let mut offenders: Vec<String> = Vec::new();

    for module in modules() {
        if may_build_terminal_progress(&module.repo_relative) {
            continue;
        }
        for (line_number, line) in constructing_lines(&module.source) {
            offenders.push(format!("  {}:{line_number}: {line}", module.repo_relative));
        }
    }

    assert!(
        offenders.is_empty(),
        "a runner built its own Progress instead of taking one \u{2014} accept a          `&mut dyn Progress`, or a factory if it reports about many things, and          let the command construct it (ADR-0047):\n{}",
        offenders.join("\n")
    );
}

/// The scan above passes by finding nothing outside the command layer, so it
/// has to be shown that it finds something inside it — otherwise a needle that
/// stopped matching (a rename, a re-export under another name) would read as
/// compliance across the whole crate.
#[test]
fn the_command_layer_still_builds_one() {
    let walked = modules();
    let building: Vec<&str> = walked
        .iter()
        .filter(|module| module.repo_relative.starts_with("src/commands/"))
        .filter(|module| !constructing_lines(&module.source).is_empty())
        .map(|module| module.repo_relative.as_str())
        .collect();

    assert!(
        !building.is_empty(),
        "no command constructs a TerminalProgress \u{2014} either the needle          `{TERMINAL_PROGRESS}` no longer matches the code, or the terminal          renderer moved and this fence is scanning for nothing"
    );
}
