//! `auberge deploy` offers every standalone playbook the tree holds, minus a
//! declared few.
//!
//! `deploy` validated a requested name against the `apps.yml` roster and
//! nothing else, so a playbook outside that roster was reachable only through
//! `auberge ansible run -t <name>` — `aoe`, `opencode` and `memsearch` all
//! were, and `ruche` would have been. The two verbs disagreed about which
//! names exist, and the one an operator reaches for first was the one that
//! knew fewer.
//!
//! Widening `deploy` to the whole standalone set makes the exclusions the
//! thing that has to be stated. They are real: `bootstrap` runs as root over
//! port 22 before the ansible user exists, and a teardown playbook is not a
//! convergence. So this is ADR-0028's declared regime — a set computed off the
//! tree matched against one a human vouched for, by equality in both
//! directions, so a new standalone playbook fails the build until it is
//! classified and a classification outliving its playbook fails until it is
//! removed.
//!
//! The domain is read through `common`'s tree walk and the classification
//! through the crate's own `standalone_stem`, so neither the walk nor the rule
//! for what counts as standalone is spelled twice (#659, ADR-0046). What is
//! *not* read from the crate is `deployable_playbooks()` itself — that is the
//! function under test, and a fence reading its own expectation asserts
//! nothing.

use std::collections::BTreeSet;

use auberge::commands::deploy::NOT_DEPLOYABLE;
use auberge::services::dependency_resolver::standalone_stem;

mod common;

use common::playbook_files;

/// Every standalone playbook `deploy` will run. Held by equality against the
/// tree minus [`NOT_DEPLOYABLE`], so adding a playbook is a visible edit here.
const DEPLOYABLE: &[&str] = &[
    "aoe",
    "calibre",
    "gokapi",
    "hermes",
    "immich",
    "memsearch",
    "opencode",
    "ruche",
];

/// Every standalone playbook in the tree, by stem.
fn standalone_playbooks() -> BTreeSet<String> {
    playbook_files()
        .iter()
        .filter_map(|path| path.file_name()?.to_str().and_then(standalone_stem))
        .map(str::to_string)
        .collect()
}

fn excluded() -> BTreeSet<String> {
    NOT_DEPLOYABLE
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect()
}

fn declared_deployable() -> BTreeSet<String> {
    DEPLOYABLE.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn test_every_standalone_playbook_is_deployable_or_says_why_not() {
    let tree = standalone_playbooks();
    let classified: BTreeSet<String> = declared_deployable().union(&excluded()).cloned().collect();

    let unclassified: Vec<&String> = tree.difference(&classified).collect();
    assert!(
        unclassified.is_empty(),
        "standalone playbook(s) nobody has classified: {unclassified:?}. Add each \
         to DEPLOYABLE, or to NOT_DEPLOYABLE in src/commands/deploy.rs with the \
         reason it is a lifecycle operation rather than a convergence"
    );

    let stale: Vec<&String> = classified.difference(&tree).collect();
    assert!(
        stale.is_empty(),
        "classification(s) naming playbooks the tree no longer holds: {stale:?}"
    );
}

#[test]
fn test_nothing_is_both_deployable_and_refused() {
    let (deployable, refused) = (declared_deployable(), excluded());
    let both: Vec<&String> = deployable.intersection(&refused).collect();
    assert!(
        both.is_empty(),
        "classified twice, contradictorily: {both:?}"
    );
}

#[test]
fn test_the_agent_tier_is_reachable_from_deploy() {
    // The acceptance criterion of #743, as far as the repo can hold it: the
    // live run is the operator's.
    for required in ["ruche", "aoe", "opencode"] {
        assert!(
            DEPLOYABLE.contains(&required),
            "`auberge deploy {required}` must resolve"
        );
    }
}

#[test]
fn test_every_exclusion_carries_the_reason_it_is_one() {
    // No test can check that a reason is true. This catches the empty string
    // and the one-word placeholder, which is the failure mode worth catching:
    // an entry added in a hurry and never argued for.
    for (name, why) in NOT_DEPLOYABLE {
        assert!(
            why.len() >= 40 && !why.contains(name),
            "NOT_DEPLOYABLE entry `{name}` restates its name or has no argument \
             behind it: {why:?}"
        );
    }
}

#[test]
fn test_bootstrap_is_never_deployable() {
    // The one exclusion that is a safety property rather than a taxonomy:
    // `deploy` connects as the ansible user on the custom SSH port and
    // prepends two plays. Against a virgin image that has neither, every one
    // of them fails before bootstrap's own play is reached.
    assert!(
        excluded().contains("bootstrap"),
        "bootstrap must stay off the deploy path"
    );
}
