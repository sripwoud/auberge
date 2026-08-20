use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybookMeta {
    #[serde(default)]
    pub required_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionPin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupRecipe>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tailnet_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub memory: HashMap<String, MemoryBudget>,
}

/// A Memory Budget: one systemd unit's `MemoryHigh=` (throttle-and-reclaim
/// ceiling) and `MemoryMax=` (OOM-kill line), declared per unit in the App's
/// Playbook Meta and injected at deploy like an App Version (ADR-0021).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryBudget {
    pub high: String,
    pub max: String,
}

/// A Pinned version: the exact value plus the upstream coordinates Renovate
/// needs to discover new releases — the shape an App Version (Playbook Meta
/// `version:` block) and a Tool Version (`# renovate:` annotation in role
/// defaults) have in common (ADR-0017). Field names match Renovate's regex
/// manager vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPin {
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

pub const ADMIN_USER_PLACEHOLDER: &str = "{admin_user}";

impl BackupRecipe {
    /// Substitute `{admin_user}` with the Host's user in every string field,
    /// so a Recipe can name per-user units and home paths (e.g. syncthing's
    /// `syncthing@<user>`) while staying pure data (ADR-0023).
    pub fn resolve(self, admin_user: &str) -> Self {
        let sub = |s: String| s.replace(ADMIN_USER_PLACEHOLDER, admin_user);
        Self {
            systemd_services: self.systemd_services.into_iter().map(sub).collect(),
            paths: self.paths.into_iter().map(sub).collect(),
            owner: self.owner.map(|(user, group)| (sub(user), sub(group))),
            db: self.db.map(|db| DbRecipe {
                name: sub(db.name),
                dump_path: sub(db.dump_path),
            }),
            post_restore_command: self.post_restore_command.map(sub),
            parameters: self
                .parameters
                .into_iter()
                .map(|(name, parameter)| {
                    (
                        name,
                        BackupParameter {
                            default: parameter.default,
                            adds_paths: parameter.adds_paths.into_iter().map(sub).collect(),
                        },
                    )
                })
                .collect(),
        }
    }

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

/// Load every committed Playbook Meta, keyed by App name.
pub fn load_all_metas(playbooks_dir: &Path) -> Result<Vec<(String, PlaybookMeta)>> {
    let entries = std::fs::read_dir(playbooks_dir).wrap_err_with(|| {
        format!(
            "Failed to read playbooks directory {}",
            playbooks_dir.display()
        )
    })?;

    let mut metas = Vec::new();
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
        metas.push((app.to_string(), PlaybookMeta::load(&path)?));
    }
    Ok(metas)
}

/// Collect every App that declares a Version, with its full upstream
/// coordinates, sorted by App name (ADR-0017).
pub fn declared_app_versions(playbooks_dir: &Path) -> Result<Vec<(String, VersionPin)>> {
    let mut versions: Vec<(String, VersionPin)> = load_all_metas(playbooks_dir)?
        .into_iter()
        .filter_map(|(app, meta)| meta.version.map(|version| (app, version)))
        .collect();
    versions.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(versions)
}

/// Collect every declared Memory Budget as `<unit>_memory_high` /
/// `<unit>_memory_max` extra-var pairs (unit-name hyphens become
/// underscores), sorted by name. Injected at deploy through the same
/// `extra_vars` seam as App Versions (ADR-0021).
pub fn app_memory_vars(playbooks_dir: &Path) -> Result<Vec<(String, String)>> {
    let mut vars = Vec::new();
    for (_, meta) in load_all_metas(playbooks_dir)? {
        for (unit, budget) in meta.memory {
            let prefix = unit.replace('-', "_");
            vars.push((format!("{prefix}_memory_high"), budget.high));
            vars.push((format!("{prefix}_memory_max"), budget.max));
        }
    }
    vars.sort();
    Ok(vars)
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

    fn role_default(role: &str, key: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("roles")
            .join(role)
            .join("defaults")
            .join("main.yml");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let defaults: HashMap<String, serde_yaml::Value> = serde_yaml::from_str(&raw)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));
        defaults
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| panic!("{role} defaults declare {key}"))
            .to_string()
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

        let install_path = role_default("paperless", "paperless_install_path");
        assert!(cmd.contains(&format!("cd {install_path}/src")));
        assert!(cmd.contains(&format!(
            "PAPERLESS_CONFIGURATION_PATH={install_path}/paperless.conf"
        )));
        assert!(cmd.contains(&format!("{install_path}/venv/bin/python3")));
    }

    #[test]
    fn test_paperless_meta_memory_budgets() {
        let memory = load_meta("paperless").memory;
        assert_eq!(memory.len(), 4);
        let task_queue = memory.get("paperless-task-queue").unwrap();
        assert_eq!(task_queue.high, "768M");
        assert_eq!(task_queue.max, "1G");
        let webserver = memory.get("paperless-webserver").unwrap();
        assert_eq!(webserver.high, "512M");
        assert_eq!(webserver.max, "768M");
        for unit in ["paperless-consumer", "paperless-scheduler"] {
            let budget = memory.get(unit).unwrap();
            assert_eq!(budget.high, "192M");
            assert_eq!(budget.max, "256M");
        }
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
            version: Some(VersionPin {
                value: "0.25.1".to_string(),
                datasource: "github-releases".to_string(),
                dep_name: "juanfont/headscale".to_string(),
                versioning: None,
                extract_version: Some("^v(?<version>.+)$".to_string()),
            }),
            backup: None,
            tailnet_only: false,
            subdomain: None,
            memory: HashMap::new(),
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

    #[test]
    fn test_resolve_substitutes_admin_user_in_every_string_field() {
        let mut parameters = HashMap::new();
        parameters.insert(
            "include_extra".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/home/{admin_user}/extra".to_string()],
            },
        );
        let recipe = BackupRecipe {
            systemd_services: vec!["syncthing@{admin_user}".to_string()],
            paths: vec!["/home/{admin_user}/.local/state/syncthing/config.xml".to_string()],
            owner: Some(("{admin_user}".to_string(), "{admin_user}".to_string())),
            db: None,
            post_restore_command: Some("chown {admin_user} /tmp/x".to_string()),
            parameters,
        };

        let resolved = recipe.resolve("alice");

        assert_eq!(resolved.systemd_services, vec!["syncthing@alice"]);
        assert_eq!(
            resolved.paths,
            vec!["/home/alice/.local/state/syncthing/config.xml"]
        );
        assert_eq!(
            resolved.owner,
            Some(("alice".to_string(), "alice".to_string()))
        );
        assert_eq!(
            resolved.post_restore_command.as_deref(),
            Some("chown alice /tmp/x")
        );
        assert_eq!(
            resolved.parameters.get("include_extra").unwrap().adds_paths,
            vec!["/home/alice/extra"]
        );
    }

    #[test]
    fn test_resolve_leaves_recipe_without_placeholders_unchanged() {
        let recipe = BackupRecipe {
            systemd_services: vec!["navidrome".to_string()],
            paths: vec!["/var/lib/navidrome".to_string()],
            owner: Some(("navidrome".to_string(), "navidrome".to_string())),
            db: Some(DbRecipe {
                name: "navidrome".to_string(),
                dump_path: "/tmp/navidrome.dump".to_string(),
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
        };

        let resolved = recipe.clone().resolve("alice");

        assert_eq!(resolved, recipe);
    }

    #[test]
    fn test_meta_memory_block_parses() {
        let yaml = r#"
required_keys: []
memory:
  paperless-task-queue: {high: 768M, max: 1G}
  paperless-webserver: {high: 512M, max: 768M}
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let budget = meta.memory.get("paperless-task-queue").unwrap();
        assert_eq!(budget.high, "768M");
        assert_eq!(budget.max, "1G");
        assert_eq!(meta.memory.len(), 2);
    }

    #[test]
    fn test_meta_without_memory_parses_to_empty() {
        let yaml = "required_keys: []\n";
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert!(meta.memory.is_empty());
    }

    #[test]
    fn test_app_memory_vars_flattens_units_to_sorted_pairs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("zeta.meta.yml"),
            "required_keys: []\nmemory:\n  zeta-worker: {high: 256M, max: 512M}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("alpha.meta.yml"),
            "required_keys: []\nmemory:\n  alpha: {high: 128M, max: 256M}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("gamma.meta.yml"), "required_keys: []\n").unwrap();

        let vars = app_memory_vars(dir.path()).unwrap();
        assert_eq!(
            vars,
            vec![
                ("alpha_memory_high".to_string(), "128M".to_string()),
                ("alpha_memory_max".to_string(), "256M".to_string()),
                ("zeta_worker_memory_high".to_string(), "256M".to_string()),
                ("zeta_worker_memory_max".to_string(), "512M".to_string()),
            ]
        );
    }

    fn parse_systemd_size(value: &str) -> Option<u64> {
        let (digits, multiplier) = match value.as_bytes().last()? {
            b'K' => (&value[..value.len() - 1], 1u64 << 10),
            b'M' => (&value[..value.len() - 1], 1u64 << 20),
            b'G' => (&value[..value.len() - 1], 1u64 << 30),
            b'T' => (&value[..value.len() - 1], 1u64 << 40),
            b'0'..=b'9' => (value, 1),
            _ => return None,
        };
        digits.parse::<u64>().ok().map(|n| n * multiplier)
    }

    #[test]
    fn test_declared_memory_budgets_use_systemd_sizes_with_high_below_max() {
        for (app, meta) in load_all_metas(&playbooks_dir()).unwrap() {
            for (unit, budget) in &meta.memory {
                let high = parse_systemd_size(&budget.high).unwrap_or_else(|| {
                    panic!("{app}: {unit} high {:?} is not a systemd size", budget.high)
                });
                let max = parse_systemd_size(&budget.max).unwrap_or_else(|| {
                    panic!("{app}: {unit} max {:?} is not a systemd size", budget.max)
                });
                assert!(
                    high <= max,
                    "{app}: {unit} declares high {} above max {}",
                    budget.high,
                    budget.max
                );
            }
        }
    }

    fn role_template_bodies() -> Vec<(std::path::PathBuf, String)> {
        let roles_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("roles");
        let mut bodies = Vec::new();
        for role in std::fs::read_dir(&roles_dir).unwrap() {
            let templates = role.unwrap().path().join("templates");
            let Ok(entries) = std::fs::read_dir(&templates) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_file() {
                    let body = std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
                    bodies.push((path, body));
                }
            }
        }
        bodies
    }

    #[test]
    fn test_grimmory_meta_memory_budget() {
        let memory = load_meta("grimmory").memory;
        let budget = memory.get("grimmory").unwrap();
        assert_eq!(budget.high, "1100M");
        assert_eq!(budget.max, "1200M");
    }

    #[test]
    fn test_navidrome_meta_memory_budget() {
        let memory = load_meta("navidrome").memory;
        let budget = memory.get("navidrome").unwrap();
        assert_eq!(budget.high, "256M");
        assert_eq!(budget.max, "384M");
    }

    #[test]
    fn test_radio_meta_is_a_public_app_with_no_version_and_no_backup() {
        let meta = load_meta("radio");
        assert_eq!(meta.subdomain.as_deref(), Some("radio"));
        assert!(!meta.tailnet_only);
        assert!(meta.version.is_none());
        assert!(meta.backup.is_none());
        assert!(
            meta.required_keys
                .contains(&"radio_listener_password".to_string())
        );
    }

    #[test]
    fn test_radio_meta_memory_budgets() {
        let memory = load_meta("radio").memory;
        assert_eq!(memory.len(), 2);
        let liquidsoap = memory.get("liquidsoap").unwrap();
        assert_eq!(liquidsoap.high, "320M");
        assert_eq!(liquidsoap.max, "384M");
        let icecast = memory.get("icecast2").unwrap();
        assert_eq!(icecast.high, "32M");
        assert_eq!(icecast.max, "64M");
    }

    #[test]
    fn test_unit_templates_carry_no_literal_memory_directives() {
        for (path, body) in role_template_bodies() {
            for line in body.lines() {
                if line.starts_with("MemoryHigh=") || line.starts_with("MemoryMax=") {
                    assert!(
                        line.contains("{{"),
                        "{}: literal {line:?} — declare it in the Playbook Meta instead (ADR-0021)",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn test_declared_memory_budgets_render_into_role_templates() {
        let templates = role_template_bodies();
        for (app, meta) in load_all_metas(&playbooks_dir()).unwrap() {
            for unit in meta.memory.keys() {
                let prefix = unit.replace('-', "_");
                let high = format!("MemoryHigh={{{{ {prefix}_memory_high }}}}");
                let max = format!("MemoryMax={{{{ {prefix}_memory_max }}}}");
                assert!(
                    templates
                        .iter()
                        .any(|(_, body)| body.contains(&high) && body.contains(&max)),
                    "{app}: no role template renders both {high} and {max}"
                );
            }
        }
    }
}
