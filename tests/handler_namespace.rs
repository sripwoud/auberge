use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

fn roles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles")
}

/// What a handler does, as a comparable string: every key but its name, with
/// `daemon_reload` dropped — reloading systemd's unit cache changes neither the
/// unit acted on nor whether the handler runs.
fn effect(handler: &serde_yaml::Mapping) -> String {
    let mut parts: Vec<String> = handler
        .iter()
        .filter(|(key, _)| key.as_str() != Some("name"))
        .map(|(key, value)| {
            let mut value = value.clone();
            if let Some(args) = value.as_mapping_mut() {
                args.remove(serde_yaml::Value::from("daemon_reload"));
            }
            format!(
                "{}={}",
                key.as_str().unwrap_or_default(),
                serde_yaml::to_string(&value)
                    .unwrap()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect();
    parts.sort();
    parts.join(" ")
}

/// Handler name -> role -> effect, across every role in the tree.
fn handler_effects() -> BTreeMap<String, BTreeMap<String, String>> {
    let mut effects: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

    for entry in fs::read_dir(roles_dir()).expect("ansible/roles must exist") {
        let role_dir = entry.unwrap().path();
        let path = role_dir.join("handlers/main.yml");
        if !path.exists() {
            continue;
        }
        let role = role_dir.file_name().unwrap().to_string_lossy().to_string();
        let handlers: Option<Vec<serde_yaml::Mapping>> =
            serde_yaml::from_str(&fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        for handler in handlers.unwrap_or_default() {
            let name = handler
                .get(serde_yaml::Value::from("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_else(|| panic!("{}: handler without a name", path.display()))
                .to_string();
            effects
                .entry(name)
                .or_default()
                .insert(role.clone(), effect(&handler));
        }
    }

    effects
}

/// Ansible handler names live in one global namespace per play: when two roles
/// co-loaded by a playbook define the same name, the last one loaded shadows
/// the rest. Sharing a name is only safe when the definitions do the same
/// thing — a shadow with a divergent `when:` guard silently skips, so the
/// notify becomes a no-op instead of an error (issue #569).
#[test]
fn test_same_named_handlers_across_roles_have_the_same_effect() {
    let violations: Vec<String> = handler_effects()
        .iter()
        .filter(|(_, by_role)| by_role.values().collect::<BTreeSet<_>>().len() > 1)
        .map(|(name, by_role)| {
            let definitions: String = by_role
                .iter()
                .map(|(role, effect)| format!("\n    {role}: {effect}"))
                .collect();
            format!(
                "handler `{name}` does something different per role, so the last role \
                 loaded shadows the others — give each one a role-scoped name:{definitions}"
            )
        })
        .collect();

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
