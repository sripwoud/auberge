use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybookMeta {
    #[serde(default)]
    pub required_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupRecipe>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tailnet_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_deploy_setup: Option<FirstDeploySetup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FirstDeploySetup {
    pub port: u16,
    pub marker_path: String,
    pub setup_url_path: String,
    pub wizard_name: String,
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
    fn test_gokapi_meta_first_deploy_setup() {
        let setup = load_meta("gokapi")
            .first_deploy_setup
            .expect("gokapi declares first_deploy_setup");
        assert_eq!(setup.port, 53842);
        assert_eq!(setup.marker_path, "/var/lib/gokapi/config/config.json");
        assert_eq!(setup.setup_url_path, "/setup");
        assert_eq!(setup.wizard_name, "Gokapi setup wizard");
    }

    #[test]
    fn test_meta_without_first_deploy_setup_parses_to_none() {
        let yaml = "required_keys: []\n";
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert!(meta.first_deploy_setup.is_none());
    }

    #[test]
    fn test_first_deploy_setup_all_fields_parse() {
        let yaml = r#"
required_keys: []
first_deploy_setup:
  port: 8443
  marker_path: /var/lib/app/.bootstrap_done
  setup_url_path: /install
  wizard_name: App setup
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let setup = meta.first_deploy_setup.expect("first_deploy_setup present");
        assert_eq!(setup.port, 8443);
        assert_eq!(setup.marker_path, "/var/lib/app/.bootstrap_done");
        assert_eq!(setup.setup_url_path, "/install");
        assert_eq!(setup.wizard_name, "App setup");
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
