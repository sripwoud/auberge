use crate::ansible_assets::AnsibleAssets;
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

fn build_tag_playbook_map() -> Result<HashMap<String, Vec<PathBuf>>> {
    let playbooks_dir = AnsibleAssets::prepare()?.playbooks_dir();
    let mut tag_map: HashMap<String, Vec<PathBuf>> = HashMap::new();

    let target_playbooks = ["hardening.yml", "infrastructure.yml", "apps.yml"];
    for filename in &target_playbooks {
        let path = playbooks_dir.join(filename);
        if !path.exists() {
            continue;
        }

        let canonical = std::fs::canonicalize(&path)
            .wrap_err_with(|| format!("Failed to canonicalize: {}", path.display()))?;

        let roles = parse_playbook_roles(&canonical)?;
        for (role_name, tags) in roles {
            tag_map
                .entry(role_name)
                .or_default()
                .push(canonical.clone());
            for tag in tags {
                let entry = tag_map.entry(tag).or_default();
                if !entry.contains(&canonical) {
                    entry.push(canonical.clone());
                }
            }
        }
    }

    Ok(tag_map)
}

const PLAYBOOK_ORDER: &[&str] = &["hardening.yml", "infrastructure.yml", "apps.yml"];

pub fn get_app_names() -> Result<Vec<String>> {
    let playbooks_dir = AnsibleAssets::prepare()?.playbooks_dir();
    let apps_path = playbooks_dir.join("apps.yml");
    if !apps_path.exists() {
        return Ok(Vec::new());
    }
    let canonical = std::fs::canonicalize(&apps_path)
        .wrap_err_with(|| format!("Failed to canonicalize: {}", apps_path.display()))?;
    let roles = parse_playbook_roles(&canonical)?;
    Ok(roles.into_iter().map(|(name, _)| name).collect())
}

pub fn resolve_tags_to_playbook_runs(tags: &[String]) -> Result<(Vec<PlaybookRun>, Vec<String>)> {
    let tag_map = build_tag_playbook_map()?;

    let mut playbook_tags: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut has_apps = false;
    let mut has_infra = false;
    let mut unknown_tags: Vec<String> = Vec::new();

    for tag in tags {
        if let Some(playbook_paths) = tag_map.get(tag) {
            for playbook_path in playbook_paths {
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
        } else {
            unknown_tags.push(tag.clone());
        }
    }

    if has_apps && !has_infra {
        let playbooks_dir = AnsibleAssets::prepare()?.playbooks_dir();
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

    Ok((runs, unknown_tags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_playbook_roles_apps() {
        let playbooks_dir = AnsibleAssets::prepare().unwrap().playbooks_dir();
        let apps_path = std::fs::canonicalize(playbooks_dir.join("apps.yml")).unwrap();
        let roles = parse_playbook_roles(&apps_path).unwrap();

        let role_names: Vec<&str> = roles.iter().map(|(name, _)| name.as_str()).collect();
        assert!(role_names.contains(&"paperless"));
        assert!(role_names.contains(&"baikal"));
        assert!(role_names.contains(&"freshrss"));
    }

    #[test]
    fn test_parse_playbook_roles_infrastructure() {
        let playbooks_dir = AnsibleAssets::prepare().unwrap().playbooks_dir();
        let infra_path = std::fs::canonicalize(playbooks_dir.join("infrastructure.yml")).unwrap();
        let roles = parse_playbook_roles(&infra_path).unwrap();

        let role_names: Vec<&str> = roles.iter().map(|(name, _)| name.as_str()).collect();
        assert!(role_names.contains(&"caddy"));
        assert!(role_names.contains(&"tailscale"));
    }

    #[test]
    fn test_build_tag_playbook_map() {
        let map = build_tag_playbook_map().unwrap();

        let paperless_playbooks = map.get("paperless").unwrap();
        assert_eq!(paperless_playbooks.len(), 1);
        assert_eq!(
            paperless_playbooks[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "apps.yml"
        );

        let caddy_playbooks = map.get("caddy").unwrap();
        assert_eq!(caddy_playbooks.len(), 1);
        assert_eq!(
            caddy_playbooks[0].file_name().unwrap().to_str().unwrap(),
            "infrastructure.yml"
        );
    }

    #[test]
    fn test_build_tag_playbook_map_overlapping_tags() {
        let map = build_tag_playbook_map().unwrap();

        let network_playbooks = map.get("network").unwrap();
        assert_eq!(network_playbooks.len(), 2);
        let filenames: Vec<&str> = network_playbooks
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(filenames.contains(&"infrastructure.yml"));
        assert!(filenames.contains(&"apps.yml"));

        let web_playbooks = map.get("web").unwrap();
        assert_eq!(web_playbooks.len(), 2);
        let filenames: Vec<&str> = web_playbooks
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(filenames.contains(&"infrastructure.yml"));
        assert!(filenames.contains(&"apps.yml"));
    }

    #[test]
    fn test_resolve_app_tag_includes_infrastructure() {
        let (runs, unknown) = resolve_tags_to_playbook_runs(&["paperless".to_string()]).unwrap();

        assert!(unknown.is_empty());
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
        let (runs, unknown) = resolve_tags_to_playbook_runs(&["tailscale".to_string()]).unwrap();

        assert!(unknown.is_empty());
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].path.file_name().unwrap().to_str().unwrap(),
            "infrastructure.yml"
        );
        assert_eq!(runs[0].tags, vec!["tailscale"]);
    }

    #[test]
    fn test_resolve_mixed_tags_ordered() {
        let (runs, _) =
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

    #[test]
    fn test_resolve_overlapping_tag_hits_both_playbooks() {
        let (runs, unknown) = resolve_tags_to_playbook_runs(&["network".to_string()]).unwrap();

        assert!(unknown.is_empty());
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].path.file_name().unwrap().to_str().unwrap(),
            "infrastructure.yml"
        );
        assert!(runs[0].tags.contains(&"network".to_string()));
        assert_eq!(
            runs[1].path.file_name().unwrap().to_str().unwrap(),
            "apps.yml"
        );
        assert!(runs[1].tags.contains(&"network".to_string()));
    }

    #[test]
    fn test_parse_playbook_roles_hardening() {
        let playbooks_dir = AnsibleAssets::prepare().unwrap().playbooks_dir();
        let hardening_path = std::fs::canonicalize(playbooks_dir.join("hardening.yml")).unwrap();
        let roles = parse_playbook_roles(&hardening_path).unwrap();

        let role_names: Vec<&str> = roles.iter().map(|(name, _)| name.as_str()).collect();
        assert!(role_names.contains(&"fail2ban"));
        assert!(role_names.contains(&"kernel_hardening"));
    }

    #[test]
    fn test_build_tag_playbook_map_includes_hardening() {
        let map = build_tag_playbook_map().unwrap();

        let fail2ban_playbooks = map.get("fail2ban").unwrap();
        assert_eq!(fail2ban_playbooks.len(), 1);
        assert_eq!(
            fail2ban_playbooks[0].file_name().unwrap().to_str().unwrap(),
            "hardening.yml"
        );
    }

    #[test]
    fn test_resolve_hardening_tag_no_infra_or_apps() {
        let (runs, unknown) = resolve_tags_to_playbook_runs(&["fail2ban".to_string()]).unwrap();

        assert!(unknown.is_empty());
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].path.file_name().unwrap().to_str().unwrap(),
            "hardening.yml"
        );
        assert_eq!(runs[0].tags, vec!["fail2ban"]);
    }

    #[test]
    fn test_resolve_unknown_tags_reported() {
        let (runs, unknown) = resolve_tags_to_playbook_runs(&["paperles".to_string()]).unwrap();

        assert!(runs.is_empty());
        assert_eq!(unknown, vec!["paperles"]);
    }
}
