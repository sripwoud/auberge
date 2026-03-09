use crate::playbooks::PlaybookManager;
use eyre::{Result, WrapErr};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PlaybookRun {
    pub path: PathBuf,
    pub tags: Vec<String>,
}

fn parse_playbook_roles(playbook_path: &PathBuf) -> Result<Vec<(String, Vec<String>)>> {
    let content = std::fs::read_to_string(playbook_path)
        .wrap_err_with(|| format!("Failed to read playbook: {}", playbook_path.display()))?;

    let docs: Vec<serde_yaml::Value> = serde_yaml::from_str(&content)
        .wrap_err_with(|| format!("Failed to parse playbook: {}", playbook_path.display()))?;

    let mut roles = Vec::new();
    for doc in &docs {
        if let Some(play_roles) = doc.get("roles").and_then(|r| r.as_sequence()) {
            for role in play_roles {
                let role_name = role
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or_default()
                    .to_string();

                let tags: Vec<String> = role
                    .get("tags")
                    .and_then(|t| t.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                if !role_name.is_empty() {
                    roles.push((role_name, tags));
                }
            }
        }
    }

    Ok(roles)
}

fn build_tag_playbook_map() -> Result<HashMap<String, PathBuf>> {
    let playbooks_dir = PlaybookManager::get_playbooks_dir()?;
    let mut tag_map: HashMap<String, PathBuf> = HashMap::new();

    let target_playbooks = ["infrastructure.yml", "apps.yml"];
    for filename in &target_playbooks {
        let path = playbooks_dir.join(filename);
        if !path.exists() {
            continue;
        }

        let canonical = std::fs::canonicalize(&path)
            .wrap_err_with(|| format!("Failed to canonicalize: {}", path.display()))?;

        let roles = parse_playbook_roles(&canonical)?;
        for (role_name, tags) in roles {
            tag_map.insert(role_name, canonical.clone());
            for tag in tags {
                tag_map.entry(tag).or_insert_with(|| canonical.clone());
            }
        }
    }

    Ok(tag_map)
}

const PLAYBOOK_ORDER: &[&str] = &["infrastructure.yml", "apps.yml"];

pub fn resolve_tags_to_playbook_runs(tags: &[String]) -> Result<Vec<PlaybookRun>> {
    let tag_map = build_tag_playbook_map()?;

    let mut playbook_tags: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut has_apps = false;
    let mut has_infra = false;

    for tag in tags {
        if let Some(playbook_path) = tag_map.get(tag) {
            let filename = playbook_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if filename == "apps.yml" {
                has_apps = true;
            }
            if filename == "infrastructure.yml" {
                has_infra = true;
            }

            playbook_tags
                .entry(playbook_path.clone())
                .or_default()
                .push(tag.clone());
        }
    }

    if has_apps && !has_infra {
        let playbooks_dir = PlaybookManager::get_playbooks_dir()?;
        let infra = playbooks_dir.join("infrastructure.yml");
        if infra.exists() {
            let canonical = std::fs::canonicalize(&infra)?;
            playbook_tags.entry(canonical).or_default();
        }
    }

    let mut runs: Vec<PlaybookRun> = Vec::new();
    for playbook_name in PLAYBOOK_ORDER {
        for (path, tags) in &playbook_tags {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if filename == *playbook_name {
                runs.push(PlaybookRun {
                    path: path.clone(),
                    tags: tags.clone(),
                });
            }
        }
    }

    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_playbook_roles_apps() {
        let playbooks_dir = PlaybookManager::get_playbooks_dir().unwrap();
        let apps_path = std::fs::canonicalize(playbooks_dir.join("apps.yml")).unwrap();
        let roles = parse_playbook_roles(&apps_path).unwrap();

        let role_names: Vec<&str> = roles.iter().map(|(name, _)| name.as_str()).collect();
        assert!(role_names.contains(&"paperless"));
        assert!(role_names.contains(&"baikal"));
        assert!(role_names.contains(&"freshrss"));
    }

    #[test]
    fn test_parse_playbook_roles_infrastructure() {
        let playbooks_dir = PlaybookManager::get_playbooks_dir().unwrap();
        let infra_path = std::fs::canonicalize(playbooks_dir.join("infrastructure.yml")).unwrap();
        let roles = parse_playbook_roles(&infra_path).unwrap();

        let role_names: Vec<&str> = roles.iter().map(|(name, _)| name.as_str()).collect();
        assert!(role_names.contains(&"caddy"));
        assert!(role_names.contains(&"tailscale"));
    }

    #[test]
    fn test_build_tag_playbook_map() {
        let map = build_tag_playbook_map().unwrap();

        let paperless_playbook = map.get("paperless").unwrap();
        assert!(paperless_playbook.file_name().unwrap().to_str().unwrap() == "apps.yml");

        let caddy_playbook = map.get("caddy").unwrap();
        assert!(caddy_playbook.file_name().unwrap().to_str().unwrap() == "infrastructure.yml");
    }

    #[test]
    fn test_resolve_app_tag_includes_infrastructure() {
        let runs = resolve_tags_to_playbook_runs(&["paperless".to_string()]).unwrap();

        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].path.file_name().unwrap().to_str().unwrap(),
            "infrastructure.yml"
        );
        assert!(runs[0].tags.is_empty());
        assert_eq!(
            runs[1].path.file_name().unwrap().to_str().unwrap(),
            "apps.yml"
        );
        assert_eq!(runs[1].tags, vec!["paperless"]);
    }

    #[test]
    fn test_resolve_infra_tag_no_apps() {
        let runs = resolve_tags_to_playbook_runs(&["tailscale".to_string()]).unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].path.file_name().unwrap().to_str().unwrap(),
            "infrastructure.yml"
        );
        assert_eq!(runs[0].tags, vec!["tailscale"]);
    }

    #[test]
    fn test_resolve_mixed_tags_ordered() {
        let runs =
            resolve_tags_to_playbook_runs(&["tailscale".to_string(), "paperless".to_string()])
                .unwrap();

        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].path.file_name().unwrap().to_str().unwrap(),
            "infrastructure.yml"
        );
        assert_eq!(
            runs[1].path.file_name().unwrap().to_str().unwrap(),
            "apps.yml"
        );
    }
}
