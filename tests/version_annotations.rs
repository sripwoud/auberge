use std::fs;
use std::path::{Path, PathBuf};

mod common;

use common::{all_roles, playbooks_dir, repo, role_dir};

/// Tool Versions: build/runtime inputs a role needs, not App identities.
/// They stay in `defaults/main.yml` with a `# renovate:` annotation.
/// A new `_version:` variable in defaults must either be an App Version
/// (declare it in the App's `<app>.meta.yml` `version:` block instead) or
/// be added here as a deliberate Tool Version decision (ADR-0017).
const TOOL_VERSIONS: &[&str] = &[
    "blocky_lego_version",
    "caddy_cloudflare_plugin_version",
    "caddy_l4_version",
    "grimmory_java_version",
    "hermes_uv_version",
    "tgtg_uv_version",
];

fn is_version_variable(line: &str) -> Option<&str> {
    let (key, _) = line.split_once(':')?;
    (key.ends_with("_version")
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
    .then_some(key)
}

/// Every `defaults/main.yml` in the tree. A role without one contributes
/// nothing, which is the same answer as a role whose defaults declare no
/// version — and both readers below are membership tests, so the order roles
/// arrive in is immaterial.
fn defaults_files() -> Vec<PathBuf> {
    all_roles()
        .iter()
        .map(|role| role_dir(role).join("defaults/main.yml"))
        .filter(|path| path.exists())
        .collect()
}

/// Every `<app>.meta.yml` that declares a `version:` block, as
/// `(app, version mapping)` pairs.
fn meta_declared_versions() -> Vec<(String, serde_yaml::Mapping)> {
    let mut versions = Vec::new();
    for entry in fs::read_dir(playbooks_dir()).expect("ansible/playbooks must exist") {
        let path = entry.unwrap().path();
        let Some(app) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".meta.yml"))
        else {
            continue;
        };
        let meta: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        if let Some(version) = meta.get("version") {
            versions.push((
                app.to_string(),
                version
                    .as_mapping()
                    .unwrap_or_else(|| panic!("{app}.meta.yml `version:` is not a mapping"))
                    .clone(),
            ));
        }
    }
    versions.sort_by(|a, b| a.0.cmp(&b.0));
    versions
}

fn contains_token(content: &str, token: &str) -> bool {
    let is_word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    content.match_indices(token).any(|(i, _)| {
        let before_ok = !content[..i].chars().next_back().is_some_and(is_word);
        let after_ok = !content[i + token.len()..]
            .chars()
            .next()
            .is_some_and(is_word);
        before_ok && after_ok
    })
}

fn role_references(role_dir: &Path, token: &str) -> bool {
    fn walk(dir: &Path, token: &str) -> bool {
        for entry in fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_dir() {
                if walk(&path, token) {
                    return true;
                }
            } else if let Ok(content) = fs::read_to_string(&path)
                && contains_token(&content, token)
            {
                return true;
            }
        }
        false
    }
    walk(role_dir, token)
}

#[test]
fn test_every_defaults_version_variable_is_an_annotated_tool_version() {
    let mut found = Vec::new();
    let mut violations = Vec::new();

    for defaults in defaults_files() {
        let content = fs::read_to_string(&defaults).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let Some(key) = is_version_variable(line) else {
                continue;
            };
            found.push(key.to_string());
            if !TOOL_VERSIONS.contains(&key) {
                violations.push(format!(
                    "{}: {key} is not a known Tool Version — an App Version \
                     belongs in the App's `<app>.meta.yml` `version:` block (ADR-0017)",
                    defaults.display()
                ));
            }
            let annotated = i > 0
                && lines[i - 1].starts_with("# renovate: datasource=")
                && lines[i - 1].contains(" depName=");
            if !annotated {
                violations.push(format!(
                    "{}: {key} is missing a `# renovate: datasource=… depName=…` annotation \
                     (see renovate.json customManagers)",
                    defaults.display()
                ));
            }
        }
    }

    found.sort();
    assert_eq!(
        found, TOOL_VERSIONS,
        "Tool Versions in role defaults diverged from the allowlist"
    );
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn test_app_versions_live_only_in_playbook_meta() {
    let defaults_keys: Vec<String> = defaults_files()
        .iter()
        .flat_map(|defaults| {
            let content = fs::read_to_string(defaults).unwrap();
            content
                .lines()
                .filter_map(|line| is_version_variable(line).map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect();

    let duplicated: Vec<String> = meta_declared_versions()
        .iter()
        .map(|(app, _)| format!("{app}_version"))
        .filter(|key| defaults_keys.contains(key))
        .collect();

    assert!(
        duplicated.is_empty(),
        "App Versions declared in a Playbook Meta must not also be defined in role \
         defaults — the value lives in exactly one file (ADR-0017): {}",
        duplicated.join(", ")
    );
}

#[test]
fn test_every_versioned_role_declares_an_app_version_in_its_meta() {
    let declared: Vec<String> = meta_declared_versions()
        .into_iter()
        .map(|(app, _)| app)
        .collect();
    let mut violations = Vec::new();

    for role in all_roles() {
        let version_var = format!("{role}_version");
        if !role_references(&role_dir(&role), &version_var) {
            continue;
        }
        if !declared.contains(&role) {
            violations.push(format!(
                "role `{role}` references {{{{ {version_var} }}}} but \
                 {role}.meta.yml declares no `version:` block"
            ));
        }
    }

    for app in &declared {
        let version_var = format!("{app}_version");
        if !role_references(&role_dir(app), &version_var) {
            violations.push(format!(
                "{app}.meta.yml declares a version but role `{app}` never \
                 references {{{{ {version_var} }}}}"
            ));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn test_renovate_manager_matches_every_declared_app_version() {
    let renovate: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo().join("renovate.json")).unwrap()).unwrap();
    let manager = renovate["customManagers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| {
            m["managerFilePatterns"][0]
                .as_str()
                .unwrap_or("")
                .contains("playbooks")
        })
        .expect("renovate.json must carry a customManager for playbook metas");

    let file_pattern = manager["managerFilePatterns"][0]
        .as_str()
        .unwrap()
        .trim_matches('/');
    let file_regex = regex::Regex::new(file_pattern).unwrap();
    let match_string = regex::Regex::new(manager["matchStrings"][0].as_str().unwrap()).unwrap();

    let versions = meta_declared_versions();
    assert!(
        versions.len() >= 11,
        "expected at least 11 declared App Versions, found {}",
        versions.len()
    );

    for (app, declared) in versions {
        let rel_path = format!("ansible/playbooks/{app}.meta.yml");
        assert!(
            file_regex.is_match(&rel_path),
            "{rel_path} escapes the renovate managerFilePatterns"
        );

        let content = fs::read_to_string(repo().join(&rel_path)).unwrap();
        let captures = match_string.captures(&content).unwrap_or_else(|| {
            panic!("{app}.meta.yml `version:` block is invisible to the renovate matchString")
        });

        let field = |key: &str| {
            declared
                .get(serde_yaml::Value::String(key.to_string()))
                .and_then(|v| v.as_str().map(str::to_string))
        };
        assert_eq!(
            captures
                .name("currentValue")
                .map(|m| m.as_str().to_string()),
            field("value"),
            "{app}: currentValue capture diverges from `value:`"
        );
        assert_eq!(
            captures.name("datasource").map(|m| m.as_str().to_string()),
            field("datasource"),
            "{app}: datasource capture diverges"
        );
        assert_eq!(
            captures.name("depName").map(|m| m.as_str().to_string()),
            field("depName"),
            "{app}: depName capture diverges"
        );
        assert_eq!(
            captures.name("versioning").map(|m| m.as_str().to_string()),
            field("versioning"),
            "{app}: versioning capture diverges"
        );
        assert_eq!(
            captures
                .name("extractVersion")
                .map(|m| m.as_str().to_string()),
            field("extractVersion"),
            "{app}: extractVersion capture diverges"
        );
    }
}
