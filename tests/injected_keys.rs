//! A key the CLI injects is demanded of no Playbook Meta, and the roster role
//! that reads it is the one the CLI gates its round trip on.
//!
//! `tailscale_authkey` was `infrastructure.meta.yml`'s third `required_key` —
//! unconditionally, every run, forever — while the role reading it does so
//! only `when: not tailscale_is_authenticated`. The value is one-shot with a
//! TTL, so what Preflight demanded was a string guaranteed meaningless the
//! moment after its single use, and both authkey-enrolled nodes held spent
//! ones. #768 moved the mint into the CLI; these hold the arrangement that
//! makes that safe.
//!
//! Read as text off the tree rather than through the resolver, like every
//! fence here: the resolver's unit tests prove it unions correctly, and this
//! proves the declarations it unions over do not contradict the CLI.

mod common;

use auberge::commands::headscale::{
    ENROLLED_STATES, ENROLLING_ROLE, ENROLLMENT_PROBE, INJECTED_AUTHKEY,
};
use common::{meta_files, parse_yaml, playbooks_dir, relative, repo};
use serde_yaml::Value;
use std::collections::BTreeSet;

/// Every key the Key Registry marks `injected:` — the CLI supplies it, so
/// `config.toml` is an override rather than the source.
fn injected_keys() -> BTreeSet<String> {
    let path = repo().join("ansible").join("keys.yml");
    let registry = parse_yaml(&path);
    let keys = registry
        .get("keys")
        .and_then(Value::as_mapping)
        .unwrap_or_else(|| panic!("{} must hold a keys: mapping", relative(&path)));
    keys.iter()
        .filter(|(_, entry)| {
            entry
                .get("injected")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|(name, _)| name.as_str().map(str::to_string))
        .collect()
}

/// The domain this fence reads, dumped so a narrowed scan cannot pass
/// vacuously: an `injected:` that nothing parses would make every assertion
/// below hold over the empty set.
#[test]
fn the_registry_declares_at_least_one_injected_key() {
    let injected = injected_keys();
    assert!(
        injected.contains(INJECTED_AUTHKEY),
        "the CLI injects {INJECTED_AUTHKEY}, so keys.yml must mark it injected: {injected:?}"
    );
}

/// The invariant #768 exists to establish. A Preflight that demands what the
/// CLI is about to hand the play fails runs on config the run never needed —
/// and for a one-shot credential, on config that cannot be kept correct.
#[test]
fn no_playbook_meta_demands_an_injected_key() {
    let injected = injected_keys();
    let mut offences = Vec::new();

    for (app, path) in meta_files() {
        let Value::Mapping(meta) = parse_yaml(&path) else {
            continue;
        };
        let declared = meta
            .get(Value::from("required_keys"))
            .and_then(Value::as_sequence)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for key in declared.iter().filter_map(Value::as_str) {
            if injected.contains(key) {
                offences.push(format!("{app} ({}) demands {key}", relative(&path)));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "an injected key is supplied by the CLI and must be demanded of no run:\n  {}",
        offences.join("\n  ")
    );
}

/// The CLI mints only for a run entering [`ENROLLING_ROLE`], so a rename of
/// that role would silently stop every mint while leaving the code compiling
/// and every other test green. Held against the roster the gate reads.
#[test]
fn the_enrolling_role_is_in_the_infrastructure_roster() {
    let path = playbooks_dir().join("infrastructure.yml");
    let plays = parse_yaml(&path);
    let roles: BTreeSet<String> = plays
        .as_sequence()
        .unwrap_or_else(|| panic!("{} must be a sequence of plays", relative(&path)))
        .iter()
        .filter_map(|play| play.get("roles").and_then(Value::as_sequence))
        .flatten()
        .filter_map(|entry| match entry {
            Value::String(name) => Some(name.clone()),
            other => other
                .get("role")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
        .collect();

    assert!(
        roles.contains(ENROLLING_ROLE),
        "the auto-mint gates on the '{ENROLLING_ROLE}' role, which {} does not list: {roles:?}",
        relative(&path)
    );
}

/// …and it is unguarded, so an untagged infrastructure run reaches it. A
/// `when:` here would make the gate answer true for runs the role skips, and
/// mint a key nothing reads on every deploy.
#[test]
fn the_enrolling_role_is_unguarded() {
    let path = playbooks_dir().join("infrastructure.yml");
    let plays = parse_yaml(&path);
    let guarded = plays
        .as_sequence()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|play| play.get("roles").and_then(Value::as_sequence))
        .flatten()
        .any(|entry| {
            entry.get("role").and_then(Value::as_str) == Some(ENROLLING_ROLE)
                && entry.get("when").is_some()
        });

    assert!(
        !guarded,
        "'{ENROLLING_ROLE}' carries a when: guard in {}; the auto-mint's gate cannot evaluate it",
        relative(&path)
    );
}

/// The CLI decides whether to mint by asking the target the *same* question
/// the role asks itself. If the two drift, they disagree about whether the
/// play will consume a key: the CLI mints nothing for a node the role is about
/// to try to enroll, and the role's assert fires with no key to read.
///
/// Read out of the role's task file as text, so a change to either side has to
/// be a change to both.
#[test]
fn the_cli_probe_asks_the_roles_own_enrollment_question() {
    let path = repo()
        .join("ansible/roles/tailscale/tasks/main.yml")
        .to_path_buf();
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)));

    assert!(
        raw.contains(ENROLLMENT_PROBE),
        "the role must run `{ENROLLMENT_PROBE}`, the command the CLI probes with: {}",
        relative(&path)
    );

    let tasks: Vec<Value> = serde_yaml::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} must parse: {e}", relative(&path)));
    let fact = find_key(&tasks, "tailscale_is_authenticated").unwrap_or_else(|| {
        panic!(
            "{} must set tailscale_is_authenticated; the CLI's probe mirrors its states",
            relative(&path)
        )
    });

    for state in ENROLLED_STATES {
        assert!(
            fact.contains(&format!("\"{state}\"")),
            "the role's tailscale_is_authenticated must count \"{state}\" as enrolled, \
             as the CLI's probe does: {fact}"
        );
    }
}

/// The first scalar value under `name` anywhere in a nested YAML document.
fn find_key(node: &[Value], name: &str) -> Option<String> {
    fn walk(node: &Value, name: &str) -> Option<String> {
        match node {
            Value::Mapping(map) => {
                for (key, value) in map {
                    if key.as_str() == Some(name)
                        && let Some(scalar) = value.as_str()
                    {
                        return Some(scalar.to_string());
                    }
                    if let Some(found) = walk(value, name) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Sequence(items) => items.iter().find_map(|item| walk(item, name)),
            _ => None,
        }
    }
    node.iter().find_map(|item| walk(item, name))
}
