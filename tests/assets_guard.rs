use std::collections::BTreeMap;

mod crate_source;

use crate_source::modules;

/// The one site whose guard has to outlive a subprocess: `run_playbook` and
/// `run_bootstrap` spawn `ansible-playbook`, which reads templates and
/// `include_*` files lazily, at task runtime. Its `AnsibleAssets` must stay
/// bound for the whole function so the shared lock on the tree is still held
/// while the child reads from it.
const RUNNER: &str = "services/ansible_runner.rs";

/// Sites that discard the guard in the same expression, keeping only a path.
/// The lock is released immediately, so a concurrent invocation carrying a
/// different fingerprint may sweep the tree out from under the path. Declared,
/// not tolerated by accident: every one of these reads a file straight away, so
/// the worst outcome is a missing file and a hard error — never a play compiled
/// from one tree and rendered from another, which is what #628 was.
///
/// A new entry here needs the same argument. Anything that hands the path to a
/// child process, or holds it across user interaction, belongs in the runner's
/// shape instead: bind the `AnsibleAssets`, then use its paths.
///
/// `commands/deploy.rs` was one of these and is not any more (ADR-0045): it
/// preflights every run in the plan and then reads App Versions and Memory
/// Budgets off the tree, all from one bound `AnsibleAssets`, where before it
/// took a transient path *after* confirming the deploy with the operator —
/// exactly the across-user-interaction shape this comment sends to the runner.
const DECLARED_TRANSIENT: &[(&str, usize)] = &[
    ("services/backup/recipe.rs", 1),
    ("services/dependency_resolver.rs", 7),
    ("services/inventory.rs", 1),
];

fn transient_uses(source: &str) -> usize {
    source
        .match_indices("AnsibleAssets::prepare()")
        .filter(|&(index, matched)| {
            let rest = &source[index + matched.len()..];
            rest.starts_with("?.") || rest.starts_with(".unwrap().")
        })
        .count()
}

fn scan() -> BTreeMap<String, usize> {
    modules()
        .into_iter()
        .map(|module| (module.src_relative, transient_uses(&module.source)))
        .filter(|(_, count)| *count > 0)
        .collect()
}

#[test]
fn test_only_declared_call_sites_drop_the_assets_guard() {
    let declared: BTreeMap<String, usize> = DECLARED_TRANSIENT
        .iter()
        .map(|(file, count)| ((*file).to_string(), *count))
        .collect();

    assert_eq!(
        scan(),
        declared,
        "a call site that discards its AnsibleAssets keeps only a path a concurrent \
         sweep can delete; bind the assets for as long as the path is used, or add \
         the site to DECLARED_TRANSIENT with the argument for why the window is safe"
    );
}

#[test]
fn test_the_playbook_runner_holds_its_guard() {
    let runner = modules()
        .into_iter()
        .find(|module| module.src_relative == RUNNER)
        .unwrap_or_else(|| panic!("the walk over src/ must reach {RUNNER}"));

    assert!(
        runner.source.contains("AnsibleAssets::prepare()"),
        "{RUNNER} must prepare the assets it runs ansible-playbook against"
    );
    assert_eq!(
        transient_uses(&runner.source),
        0,
        "{RUNNER} spawns ansible-playbook, which reads templates lazily at task \
         runtime; discarding the AnsibleAssets releases the lock that keeps the \
         tree from being swept mid-play (#628)"
    );
}
