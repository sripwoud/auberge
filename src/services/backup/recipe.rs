use crate::ansible_assets::AnsibleAssets;
use crate::playbook_meta::{BackupRecipe, PlaybookMeta};
use eyre::{Result, WrapErr};
use std::path::Path;

pub fn load_app_recipe(playbooks_dir: &Path, app: &str) -> Result<BackupRecipe> {
    let meta_path = playbooks_dir.join(format!("{app}.meta.yml"));
    let meta = PlaybookMeta::load(&meta_path)
        .wrap_err_with(|| format!("Failed to load Playbook Meta for app '{app}'"))?;
    meta.backup
        .ok_or_else(|| eyre::eyre!("Playbook Meta for '{app}' has no `backup:` section"))
}

pub fn discover_backuppable_apps(playbooks_dir: &Path) -> Result<Vec<String>> {
    let mut apps = Vec::new();
    for entry in std::fs::read_dir(playbooks_dir)
        .wrap_err_with(|| format!("Failed to read playbooks dir: {}", playbooks_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let stem = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) if name.ends_with(".meta.yml") => name.trim_end_matches(".meta.yml"),
            _ => continue,
        };
        let meta = match PlaybookMeta::load(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.backup.is_some() {
            apps.push(stem.to_string());
        }
    }
    apps.sort();
    Ok(apps)
}

pub fn assets_playbooks_dir() -> Result<std::path::PathBuf> {
    Ok(AnsibleAssets::prepare()?.playbooks_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_playbooks_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("playbooks")
    }

    #[test]
    fn test_load_app_recipe_returns_baikal() {
        let recipe = load_app_recipe(&project_playbooks_dir(), "baikal").unwrap();
        assert_eq!(recipe.paths, vec!["/opt/baikal/Specific"]);
    }

    #[test]
    fn test_load_app_recipe_errors_when_meta_missing_backup_section() {
        let result = load_app_recipe(&project_playbooks_dir(), "bootstrap");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no `backup:` section")
        );
    }

    #[test]
    fn test_load_app_recipe_errors_for_unknown_app() {
        let result = load_app_recipe(&project_playbooks_dir(), "definitely-not-an-app");
        assert!(result.is_err());
    }

    #[test]
    fn test_discover_backuppable_apps_returns_all_nine() {
        let apps = discover_backuppable_apps(&project_playbooks_dir()).unwrap();
        for expected in [
            "baikal",
            "bichon",
            "calibre",
            "freshrss",
            "headscale",
            "navidrome",
            "paperless",
            "webdav",
            "yourls",
        ] {
            assert!(
                apps.contains(&expected.to_string()),
                "expected '{expected}' in discovered apps, got {apps:?}"
            );
        }
    }

    #[test]
    fn test_discover_backuppable_apps_excludes_non_backup_metas() {
        let apps = discover_backuppable_apps(&project_playbooks_dir()).unwrap();
        assert!(!apps.contains(&"bootstrap".to_string()));
        assert!(!apps.contains(&"hardening".to_string()));
        assert!(!apps.contains(&"infrastructure".to_string()));
        assert!(!apps.contains(&"hermes".to_string()));
        assert!(!apps.contains(&"vibecoder".to_string()));
        assert!(!apps.contains(&"remove-radicale".to_string()));
        assert!(!apps.contains(&"apps".to_string()));
    }
}
