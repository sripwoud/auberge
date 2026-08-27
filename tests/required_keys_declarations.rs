//! Every `required_keys` declaration is answerable, and every run has one to read.
//!
//! ADR-0045 moved Preflight validation onto the Playbook Metas, which makes the
//! declarations load-bearing in a way they were not while `config.rs` held a
//! hardcoded table beside them. Three things have to hold for a run's demand to
//! mean anything, and none of them held by construction before:
//!
//! - a declared name has to exist in the Key Registry, or the run demands a key
//!   the user can never set and `config init` never offers;
//! - every roster role has to have a Meta, or a tag selects an App whose keys go
//!   unasked;
//! - every playbook has to have a Meta, or the run validates nothing at all and
//!   says nothing about it — the silent-zero case, which is worse than the drift
//!   it replaces because a green Preflight reads as a checked one.
//!
//! Read as text off the tree rather than through the crate, like every fence
//! here: the resolver's own unit tests prove it unions correctly, and these
//! prove the tree it unions over is complete.

mod common;

use common::{meta_files, parse_yaml, playbook_files, playbooks_dir, registry_keys, relative};
use serde_yaml::{Mapping, Value};
use std::collections::BTreeSet;
use std::fs;

/// The shared parse, narrowed to the mapping every file read here must be.
fn parse(path: &std::path::Path) -> Mapping {
    match parse_yaml(path) {
        Value::Mapping(map) => map,
        other => panic!("{} must be a mapping, got {other:?}", relative(path)),
    }
}

fn declared_keys(meta: &Mapping) -> Vec<String> {
    meta.get(Value::from("required_keys"))
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The roster of `playbook`, as `(role, tags)` — the same shape the resolver
/// reads, spelled independently of it.
fn roster(playbook: &str) -> Vec<(String, Vec<String>)> {
    let path = playbooks_dir().join(playbook);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)));
    let plays: Vec<Value> = serde_yaml::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} must parse: {e}", relative(&path)));
    let mut roles = Vec::new();
    for play in &plays {
        let Some(entries) = play.get("roles").and_then(Value::as_sequence) else {
            continue;
        };
        for entry in entries {
            let Some(name) = entry.get("role").and_then(Value::as_str) else {
                continue;
            };
            roles.push((
                name.to_string(),
                entry
                    .get("tags")
                    .and_then(Value::as_sequence)
                    .map(|seq| {
                        seq.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            ));
        }
    }
    assert!(!roles.is_empty(), "{} declares no roles", relative(&path));
    roles
}

#[test]
fn every_declared_required_key_exists_in_the_key_registry() {
    let registry = registry_keys();
    let mut unknown = Vec::new();
    for (app, path) in meta_files() {
        for key in declared_keys(&parse(&path)) {
            if !registry.contains(&key) {
                unknown.push(format!("{app}: {key}"));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "Metas declare required keys absent from ansible/keys.yml, so no config \
         can ever satisfy them: {}",
        unknown.join(", ")
    );
}

#[test]
fn every_apps_roster_role_has_a_playbook_meta() {
    let missing: Vec<String> = roster("apps.yml")
        .into_iter()
        .map(|(role, _)| role)
        .filter(|role| !playbooks_dir().join(format!("{role}.meta.yml")).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "apps.yml roster roles with no Playbook Meta, so a tag selecting them \
         demands none of their keys: {}",
        missing.join(", ")
    );
}

#[test]
fn every_playbook_has_a_playbook_meta() {
    let missing: Vec<String> = playbook_files()
        .into_iter()
        .filter(|path| {
            let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            let Some(stem) = stem.strip_suffix(".yml") else {
                return false;
            };
            !playbooks_dir().join(format!("{stem}.meta.yml")).is_file()
        })
        .map(|path| relative(&path))
        .collect();
    assert!(
        missing.is_empty(),
        "playbooks with no Playbook Meta, so their runs validate nothing and \
         report a clean Preflight for it: {}",
        missing.join(", ")
    );
}

/// Every tag an operator can pass resolves to at least one role, so a typo is a
/// no-op run rather than a silently unvalidated one.
#[test]
fn every_roster_tag_selects_a_role() {
    for playbook in ["apps.yml", "infrastructure.yml"] {
        let roles = roster(playbook);
        let tags: BTreeSet<&String> = roles.iter().flat_map(|(_, tags)| tags).collect();
        for tag in tags {
            let selected = roles
                .iter()
                .filter(|(role, role_tags)| role == tag || role_tags.contains(tag))
                .count();
            assert!(selected > 0, "{playbook}: tag '{tag}' selects no role");
        }
    }
}
