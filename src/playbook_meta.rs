use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybookMeta {
    #[serde(default)]
    pub required_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<AppVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupRecipe>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tailnet_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
}

/// The App Version: the identity of the deployed App, plus the upstream
/// coordinates Renovate needs to discover new releases (ADR-0017).
/// Field names match Renovate's regex manager vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVersion {
    pub value: String,
    pub datasource: String,
    pub dep_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versioning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupRecipe {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub systemd_services: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<DbRecipe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_restore_command: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parameters: HashMap<String, BackupParameter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbRecipe {
    pub name: String,
    pub dump_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupParameter {
    #[serde(default)]
    pub default: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adds_paths: Vec<String>,
}

impl BackupRecipe {
    pub fn effective_paths(&self, parameter_values: &HashMap<String, bool>) -> Vec<String> {
        let mut paths = self.paths.clone();
        for (name, parameter) in &self.parameters {
            let value = parameter_values
                .get(name)
                .copied()
                .unwrap_or(parameter.default);
            if value {
                paths.extend(parameter.adds_paths.iter().cloned());
            }
        }
        paths
    }
}

impl PlaybookMeta {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read Playbook Meta from {}", path.display()))?;
        serde_yaml::from_str(&contents)
            .wrap_err_with(|| format!("Failed to parse Playbook Meta from {}", path.display()))
    }
}

/// Collect every App that declares a Version, with its full upstream
/// coordinates, sorted by App name (ADR-0017).
pub fn declared_app_versions(playbooks_dir: &Path) -> Result<Vec<(String, AppVersion)>> {
    let entries = std::fs::read_dir(playbooks_dir).wrap_err_with(|| {
        format!(
            "Failed to read playbooks directory {}",
            playbooks_dir.display()
        )
    })?;

    let mut versions = Vec::new();
    for entry in entries {
        let path = entry
            .wrap_err("Failed to read playbooks directory entry")?
            .path();
        let Some(app) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".meta.yml"))
        else {
            continue;
        };
        if let Some(version) = PlaybookMeta::load(&path)?.version {
            versions.push((app.to_string(), version));
        }
    }
    versions.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(versions)
}

/// Collect every declared App Version as a `<app>_version` extra-var pair,
/// sorted by name. Repo-owned data injected at deploy through `run_playbook`'s
/// `extra_vars` seam — deliberately not part of user `Config` (ADR-0017).
pub fn app_version_vars(playbooks_dir: &Path) -> Result<Vec<(String, String)>> {
    Ok(declared_app_versions(playbooks_dir)?
        .into_iter()
        .map(|(app, version)| (format!("{app}_version"), version.value))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playbooks_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("playbooks")
    }

    fn load_meta(name: &str) -> PlaybookMeta {
        let path = playbooks_dir().join(format!("{name}.meta.yml"));
        PlaybookMeta::load(&path).unwrap_or_else(|e| panic!("Failed to load {name}.meta.yml: {e}"))
    }

    #[test]
    fn test_bootstrap_meta_parses_without_error() {
        let meta = load_meta("bootstrap");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"ssh_port".to_string()));
        assert!(meta.required_keys.contains(&"hostname".to_string()));
    }

    #[test]
    fn test_hardening_meta_parses_without_error() {
        let meta = load_meta("hardening");
        assert!(meta.required_keys.is_empty());
    }

    #[test]
    fn test_infrastructure_meta_parses_without_error() {
        let meta = load_meta("infrastructure");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
        assert!(
            meta.required_keys
                .contains(&"tailscale_authkey".to_string())
        );
    }

    #[test]
    fn test_apps_meta_parses_without_error() {
        let meta = load_meta("apps");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
        assert!(
            meta.required_keys
                .contains(&"cloudflare_dns_api_token".to_string())
        );
    }

    #[test]
    fn test_hermes_meta_parses_without_error() {
        let meta = load_meta("hermes");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
        assert!(
            meta.required_keys
                .contains(&"hermes_llm_provider".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_llm_api_key".to_string())
        );
        assert!(
            meta.required_keys
                .contains(&"hermes_telegram_bot_token".to_string())
        );
    }

    #[test]
    fn test_calibre_meta_parses_without_error() {
        let meta = load_meta("calibre");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
        let backup = meta.backup.expect("calibre.meta.yml should declare backup");
        assert_eq!(backup.systemd_services, vec!["calibre"]);
        assert_eq!(
            backup.paths,
            vec!["/srv/calibre", "/opt/calibre", "/home/calibre"]
        );
        assert_eq!(
            backup.owner,
            Some(("calibre".to_string(), "calibre".to_string()))
        );
        assert!(backup.db.is_none());
    }

    #[test]
    fn test_actual_meta_backup_recipe() {
        let backup = load_meta("actual").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["actual"]);
        assert_eq!(backup.paths, vec!["/var/lib/actual"]);
        assert_eq!(
            backup.owner,
            Some(("actual".to_string(), "actual".to_string()))
        );
        assert!(backup.db.is_none());
    }

    #[test]
    fn test_actual_meta_is_tailnet_only() {
        let meta = load_meta("actual");
        assert!(meta.tailnet_only);
        assert_eq!(meta.subdomain.as_deref(), Some("actual"));
        assert!(meta.required_keys.is_empty());
    }

    #[test]
    fn test_baikal_meta_backup_recipe() {
        let backup = load_meta("baikal").backup.unwrap();
        assert_eq!(backup.paths, vec!["/opt/baikal/Specific"]);
        assert_eq!(
            backup.owner,
            Some(("baikal".to_string(), "baikal".to_string()))
        );
        assert!(backup.systemd_services.is_empty());
    }

    #[test]
    fn test_bichon_meta_backup_recipe() {
        let backup = load_meta("bichon").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["bichon"]);
        assert_eq!(backup.paths, vec!["/var/lib/bichon-archive"]);
    }

    #[test]
    fn test_freshrss_meta_backup_recipe() {
        let backup = load_meta("freshrss").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["freshrss"]);
        assert_eq!(
            backup.paths,
            vec!["/var/lib/freshrss", "/opt/freshrss/data"]
        );
    }

    #[test]
    fn test_gokapi_meta_backup_recipe() {
        let backup = load_meta("gokapi").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["gokapi"]);
        assert_eq!(backup.paths, vec!["/var/lib/gokapi"]);
        assert_eq!(
            backup.owner,
            Some(("gokapi".to_string(), "gokapi".to_string()))
        );
    }

    #[test]
    fn test_tgtg_meta_backup_recipe() {
        let meta = load_meta("tgtg");
        assert!(
            meta.required_keys
                .contains(&"tgtg_telegram_bot_token".to_string())
        );
        let backup = meta.backup.expect("tgtg.meta.yml should declare backup");
        assert_eq!(backup.systemd_services, vec!["tgtg"]);
        assert_eq!(backup.paths, vec!["/var/lib/tgtg"]);
        assert_eq!(backup.owner, Some(("tgtg".to_string(), "tgtg".to_string())));
        assert!(backup.db.is_none());
    }

    #[test]
    fn test_headscale_meta_backup_recipe() {
        let backup = load_meta("headscale").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["headscale"]);
        assert_eq!(backup.paths, vec!["/var/lib/headscale"]);
    }

    #[test]
    fn test_navidrome_meta_backup_recipe() {
        let backup = load_meta("navidrome").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["navidrome"]);
        assert_eq!(backup.paths, vec!["/var/lib/navidrome", "/etc/navidrome"]);
        let parameter = backup.parameters.get("include_music").unwrap();
        assert!(!parameter.default);
        assert_eq!(parameter.adds_paths, vec!["/srv/music"]);
    }

    #[test]
    fn test_yourls_meta_backup_recipe() {
        let backup = load_meta("yourls").backup.unwrap();
        assert_eq!(backup.paths, vec!["/var/www/yourls"]);
        assert_eq!(
            backup.owner,
            Some(("www-data".to_string(), "www-data".to_string()))
        );
    }

    #[test]
    fn test_paperless_meta_backup_recipe() {
        let backup = load_meta("paperless").backup.unwrap();
        assert_eq!(
            backup.systemd_services,
            vec![
                "paperless-webserver",
                "paperless-consumer",
                "paperless-task-queue",
                "paperless-scheduler",
            ]
        );
        assert_eq!(
            backup.paths,
            vec!["/opt/paperless/data", "/opt/paperless/media"]
        );
        let db = backup.db.expect("paperless declares db");
        assert_eq!(db.name, "paperless");
        assert_eq!(db.dump_path, "/tmp/paperless_db.dump");
        let cmd = backup
            .post_restore_command
            .expect("paperless declares post_restore_command");
        assert!(cmd.contains("manage.py migrate"));
        assert!(cmd.contains("PAPERLESS_CONFIGURATION_PATH"));
    }

    #[test]
    fn test_remove_radicale_meta_parses_without_error() {
        let meta = load_meta("remove-radicale");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
    }

    #[test]
    fn test_vibecoder_meta_parses_without_error() {
        let meta = load_meta("vibecoder");
        assert!(meta.required_keys.contains(&"admin_user_name".to_string()));
        assert!(meta.required_keys.contains(&"domain".to_string()));
    }

    #[test]
    fn test_all_committed_playbooks_have_meta_files() {
        let playbooks_dir = playbooks_dir();
        let playbook_files: Vec<_> = std::fs::read_dir(&playbooks_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("yml")
                    && !p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .ends_with(".meta")
            })
            .collect();

        assert!(
            !playbook_files.is_empty(),
            "No playbook files found in playbooks dir"
        );

        for playbook in &playbook_files {
            let stem = playbook.file_stem().and_then(|s| s.to_str()).unwrap();
            let meta_path = playbooks_dir.join(format!("{stem}.meta.yml"));
            assert!(
                meta_path.exists(),
                "Missing meta file for playbook: {stem}.yml (expected {meta_path:?})"
            );
            PlaybookMeta::load(&meta_path)
                .unwrap_or_else(|e| panic!("Failed to parse {stem}.meta.yml: {e}"));
        }
    }

    #[test]
    fn test_playbook_meta_load_nonexistent_file_returns_error() {
        let result = PlaybookMeta::load(Path::new("/nonexistent/playbook.meta.yml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_version_block_parses() {
        let yaml = r#"
version:
  value: "26.8.0"
  datasource: npm
  depName: "@actual-app/sync-server"
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let version = meta.version.unwrap();
        assert_eq!(version.value, "26.8.0");
        assert_eq!(version.datasource, "npm");
        assert_eq!(version.dep_name, "@actual-app/sync-server");
        assert!(version.versioning.is_none());
        assert!(version.extract_version.is_none());
    }

    #[test]
    fn test_meta_version_block_with_all_coordinates_parses() {
        let yaml = r#"
version:
  value: "2.3.0"
  datasource: github-releases
  depName: "sripwoud/auberge"
  versioning: loose
  extractVersion: "^grimmory/v(?<version>.+)$"
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let version = meta.version.unwrap();
        assert_eq!(version.versioning.as_deref(), Some("loose"));
        assert_eq!(
            version.extract_version.as_deref(),
            Some("^grimmory/v(?<version>.+)$")
        );
    }

    #[test]
    fn test_meta_version_block_round_trips() {
        let meta = PlaybookMeta {
            required_keys: vec![],
            version: Some(AppVersion {
                value: "0.25.1".to_string(),
                datasource: "github-releases".to_string(),
                dep_name: "juanfont/headscale".to_string(),
                versioning: None,
                extract_version: Some("^v(?<version>.+)$".to_string()),
            }),
            backup: None,
            tailnet_only: false,
            subdomain: None,
        };
        let yaml = serde_yaml::to_string(&meta).unwrap();
        let reparsed: PlaybookMeta = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(reparsed, meta);
    }

    #[test]
    fn test_meta_without_version_parses_to_none() {
        let yaml = "required_keys: []\n";
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert!(meta.version.is_none());
    }

    #[test]
    fn test_app_version_vars_harvests_only_declared_versions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("beta.meta.yml"),
            "required_keys: []\nversion:\n  value: \"1.2.3\"\n  datasource: npm\n  depName: \"beta\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("alpha.meta.yml"),
            "required_keys: []\nversion:\n  value: \"v9\"\n  datasource: github-releases\n  depName: \"a/a\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("gamma.meta.yml"), "required_keys: []\n").unwrap();
        std::fs::write(dir.path().join("apps.yml"), "---\n- hosts: vps\n").unwrap();

        let vars = app_version_vars(dir.path()).unwrap();
        assert_eq!(
            vars,
            vec![
                ("alpha_version".to_string(), "v9".to_string()),
                ("beta_version".to_string(), "1.2.3".to_string()),
            ]
        );
    }

    #[test]
    fn test_app_version_vars_injects_every_committed_app_version() {
        let vars = app_version_vars(&playbooks_dir()).unwrap();
        let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
        for expected in [
            "actual_version",
            "baikal_version",
            "bichon_version",
            "blocky_version",
            "colporteur_version",
            "freshrss_version",
            "gokapi_version",
            "grimmory_version",
            "headscale_version",
            "hermes_version",
            "navidrome_version",
            "paperless_version",
            "tgtg_version",
            "yourls_version",
        ] {
            assert!(names.contains(&expected), "missing extra var: {expected}");
        }
        assert!(vars.iter().all(|(_, value)| !value.is_empty()));
    }

    #[test]
    fn test_declared_app_versions_returns_sorted_apps_with_coordinates() {
        let versions = declared_app_versions(&playbooks_dir()).unwrap();

        let apps: Vec<&str> = versions.iter().map(|(app, _)| app.as_str()).collect();
        let mut sorted = apps.clone();
        sorted.sort();
        assert_eq!(apps, sorted);

        let (_, actual) = versions
            .iter()
            .find(|(app, _)| app == "actual")
            .expect("actual declares a version");
        assert_eq!(actual.datasource, "npm");
        assert_eq!(actual.dep_name, "@actual-app/sync-server");
        assert!(!actual.value.is_empty());
    }

    #[test]
    fn test_meta_without_backup_section_parses() {
        let yaml = "required_keys: [foo, bar]\n";
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.required_keys, vec!["foo", "bar"]);
        assert!(meta.backup.is_none());
        assert!(!meta.tailnet_only);
        assert!(meta.subdomain.is_none());
    }

    #[test]
    fn test_bichon_meta_is_tailnet_only() {
        let meta = load_meta("bichon");
        assert!(meta.tailnet_only);
        assert_eq!(meta.subdomain.as_deref(), Some("bichon"));
    }

    #[test]
    fn test_paperless_meta_is_tailnet_only() {
        let meta = load_meta("paperless");
        assert!(meta.tailnet_only);
        assert_eq!(meta.subdomain.as_deref(), Some("paperless"));
    }

    #[test]
    fn test_cockpit_meta_is_tailnet_only() {
        let meta = load_meta("cockpit");
        assert!(meta.tailnet_only);
        assert_eq!(meta.subdomain.as_deref(), Some("cockpit"));
    }

    #[test]
    fn test_public_app_meta_declares_subdomain_and_is_not_tailnet_only() {
        let meta = load_meta("freshrss");
        assert!(!meta.tailnet_only);
        assert_eq!(meta.subdomain.as_deref(), Some("freshrss"));
    }

    #[test]
    fn test_meta_without_subdomain_parses_to_none() {
        let yaml = "required_keys: []\n";
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert!(meta.subdomain.is_none());
        assert!(!meta.tailnet_only);
    }

    #[test]
    fn test_minimal_backup_recipe_parses() {
        let yaml = r#"
required_keys: []
backup:
  paths:
    - /opt/app/data
  owner: [app, app]
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let backup = meta.backup.unwrap();
        assert_eq!(backup.paths, vec!["/opt/app/data"]);
        assert_eq!(backup.owner, Some(("app".to_string(), "app".to_string())));
        assert!(backup.systemd_services.is_empty());
        assert!(backup.db.is_none());
        assert!(backup.post_restore_command.is_none());
        assert!(backup.parameters.is_empty());
    }

    #[test]
    fn test_full_backup_recipe_parses() {
        let yaml = r#"
required_keys: []
backup:
  systemd_services: [paperless-webserver, paperless-consumer]
  paths:
    - /opt/paperless/data
    - /opt/paperless/media
  owner: [paperless, paperless]
  db:
    name: paperless
    dump_path: /tmp/paperless_db.dump
  post_restore_command: "cd /opt/paperless/src && sudo -u paperless ./manage.py migrate"
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let backup = meta.backup.unwrap();
        assert_eq!(
            backup.systemd_services,
            vec!["paperless-webserver", "paperless-consumer"]
        );
        assert_eq!(
            backup.paths,
            vec!["/opt/paperless/data", "/opt/paperless/media"]
        );
        let db = backup.db.unwrap();
        assert_eq!(db.name, "paperless");
        assert_eq!(db.dump_path, "/tmp/paperless_db.dump");
        assert!(
            backup
                .post_restore_command
                .as_deref()
                .unwrap()
                .contains("manage.py migrate")
        );
    }

    #[test]
    fn test_backup_recipe_with_parameters_parses() {
        let yaml = r#"
required_keys: []
backup:
  paths:
    - /var/lib/navidrome
  owner: [navidrome, navidrome]
  parameters:
    include_music:
      default: false
      adds_paths: [/srv/music]
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let backup = meta.backup.unwrap();
        let parameter = backup.parameters.get("include_music").unwrap();
        assert!(!parameter.default);
        assert_eq!(parameter.adds_paths, vec!["/srv/music"]);
    }

    #[test]
    fn test_effective_paths_without_parameter_returns_base_paths() {
        let recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/var/lib/app".to_string()],
            owner: None,
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
        };
        let effective = recipe.effective_paths(&HashMap::new());
        assert_eq!(effective, vec!["/var/lib/app".to_string()]);
    }

    #[test]
    fn test_effective_paths_includes_optional_paths_when_parameter_true() {
        let mut parameters = HashMap::new();
        parameters.insert(
            "include_music".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        let recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/var/lib/navidrome".to_string()],
            owner: None,
            db: None,
            post_restore_command: None,
            parameters,
        };
        let mut values = HashMap::new();
        values.insert("include_music".to_string(), true);
        let effective = recipe.effective_paths(&values);
        assert!(effective.contains(&"/var/lib/navidrome".to_string()));
        assert!(effective.contains(&"/srv/music".to_string()));
    }

    #[test]
    fn test_effective_paths_excludes_optional_paths_when_parameter_false() {
        let mut parameters = HashMap::new();
        parameters.insert(
            "include_music".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        let recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/var/lib/navidrome".to_string()],
            owner: None,
            db: None,
            post_restore_command: None,
            parameters,
        };
        let effective = recipe.effective_paths(&HashMap::new());
        assert!(!effective.contains(&"/srv/music".to_string()));
    }
}
