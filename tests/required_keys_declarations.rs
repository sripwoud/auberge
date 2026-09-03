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

use common::{
    meta_files, parse_yaml, playbook_files, playbooks_dir, registry_keys, relative, repo,
};
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

/// One roster entry, as this fence reads it off the file: the role, the tags
/// that select it, and whether a `when:` guards it — the same shape the
/// resolver reads, spelled independently of it.
struct Entry {
    role: String,
    tags: Vec<String>,
    guarded: bool,
}

fn roster(playbook: &str) -> Vec<Entry> {
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
            roles.push(Entry {
                role: name.to_string(),
                tags: entry
                    .get("tags")
                    .and_then(Value::as_sequence)
                    .map(|seq| {
                        seq.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                guarded: entry.get("when").is_some(),
            });
        }
    }
    roles
}

/// [`roster`] where the caller's question presupposes there is one, so a
/// playbook that stops declaring roles fails rather than passing vacuously.
fn required_roster(playbook: &str) -> Vec<Entry> {
    let roles = roster(playbook);
    assert!(!roles.is_empty(), "{playbook} declares no roles");
    roles
}

/// The `required_keys` of `<name>.meta.yml`, and whether there was one to
/// read. A role with no Meta declares nothing, and "nothing" is the answer
/// that would let this fence pass over it.
fn meta_keys(name: &str) -> Option<Vec<String>> {
    let path = playbooks_dir().join(format!("{name}.meta.yml"));
    path.is_file().then(|| declared_keys(&parse(&path)))
}

/// Every Key Registry name a role's own YAML references, at any depth under
/// the directories ansible renders.
///
/// The registry filter is what makes a plain text scan safe here: a role's
/// internal variables are not registry names, so the only hits are keys an
/// operator sets — which is exactly the set a Meta has to demand.
fn registry_keys_a_role_reads(role: &str) -> BTreeSet<String> {
    let dir = repo().join("ansible").join("roles").join(role);
    let mut text = String::new();
    for sub in ["defaults", "tasks", "vars", "handlers", "meta", "templates"] {
        collect_text(&dir.join(sub), &mut text);
    }
    registry_keys()
        .into_iter()
        .filter(|key| text.contains(key.as_str()))
        .collect()
}

fn collect_text(dir: &std::path::Path, out: &mut String) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_text(&path, out);
        } else if let Ok(raw) = fs::read_to_string(&path) {
            out.push_str(&raw);
        }
    }
}

/// A playbook whose every roster entry is `when:`-guarded declares its roles'
/// keys itself.
///
/// ADR-0045 has an untagged run select only the unguarded entries, because a
/// guard turns on Host facts no caller can evaluate before the play runs and
/// demanding its keys would fail Hosts the role never touches. That is right
/// for `apps.yml`, which runs against every Host — and it means a playbook
/// where *everything* is guarded demands nothing at all on an untagged run.
/// `auberge deploy ruche` is exactly that run: Preflight passes on a config
/// that answers none of the agent tier's keys, and the play dies partway
/// through on the first undefined one, having already installed the rest.
///
/// Such a playbook exists only for the Host its guard names, so demanding the
/// keys unconditionally costs nothing and is what makes its Preflight mean
/// something.
#[test]
fn an_all_guarded_playbook_declares_the_keys_of_the_roles_it_guards() {
    let mut findings = Vec::new();
    for path in playbook_files() {
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = filename.strip_suffix(".yml") else {
            continue;
        };
        let entries = roster(filename);
        // A playbook with no roster (one that is all tasks) has no
        // guarded roles to demand keys for.
        if entries.is_empty() || entries.iter().any(|entry| !entry.guarded) {
            continue;
        }
        let declared: BTreeSet<String> = meta_keys(stem).unwrap_or_default().into_iter().collect();
        for entry in &entries {
            // A role with a Meta of its own states its demand there. One
            // without -- `github_identity` is the case -- states it nowhere,
            // so its own YAML is the only source, and reading the Meta alone
            // would pass over it seeing an empty list.
            let demanded = match meta_keys(&entry.role) {
                Some(keys) => keys.into_iter().collect(),
                None => registry_keys_a_role_reads(&entry.role),
            };
            for key in demanded {
                if !declared.contains(&key) {
                    findings.push(format!("{stem}: {} demands {key}", entry.role));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "every roster entry of these playbooks is guarded, so an untagged run \
         selects no role and Preflight demands nothing — each key below is one \
         the run reads and no Preflight asks for: {}",
        findings.join(", ")
    );
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
    let missing: Vec<String> = required_roster("apps.yml")
        .into_iter()
        .map(|entry| entry.role)
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
        let roles = required_roster(playbook);
        let tags: BTreeSet<&String> = roles.iter().flat_map(|entry| &entry.tags).collect();
        for tag in tags {
            let selected = roles
                .iter()
                .filter(|entry| &entry.role == tag || entry.tags.contains(tag))
                .count();
            assert!(selected > 0, "{playbook}: tag '{tag}' selects no role");
        }
    }
}
