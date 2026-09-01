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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<UnitDecl>,
}

/// A Unit Ownership entry: one systemd unit the App answers for, which is
/// what a failed deploy reads the state of (#644). A bare string is a system
/// unit; a unit that lives in a user manager says so, because `systemctl`
/// cannot see it without `--user`. Bare names mean `.service` (ADR-0032) and
/// `{admin_user}` resolves like a Recipe's (ADR-0023).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnitDecl {
    Name(String),
    Scoped { name: String, scope: UnitScope },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitScope {
    System,
    User,
}

/// A declared unit, resolved to what `systemctl` addresses: name qualified,
/// placeholder substituted, scope explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedUnit {
    pub name: String,
    pub scope: UnitScope,
}

/// systemd's own closed set of unit types. An entry carrying one of these is
/// already a unit name; anything else is a bare service name.
///
/// The two functions below read this one table to answer two different
/// questions about the same declared name, and `@` is where they part:
/// [`qualified_unit_name`] answers what `systemctl show` addresses, where an
/// instance stays an instance, and [`unit_file_name`] answers what
/// `systemctl list-unit-files` knows, where an instance collapses to the
/// template behind it. Intentional forks, not duplicates — a caller that
/// reaches for the other one gets a name systemd will not resolve, which is
/// how the restore preflight came to look for `syncthing@alice.service` among
/// unit files that only hold `syncthing@.service`.
pub const UNIT_TYPE_SUFFIXES: &[&str] = &[
    ".automount",
    ".device",
    ".mount",
    ".path",
    ".scope",
    ".service",
    ".slice",
    ".socket",
    ".swap",
    ".target",
    ".timer",
];

/// The loaded unit a declared name addresses, which is what `systemctl show`
/// answers for: an explicit unit type is kept, a bare name is a `.service`.
/// Instances stay instances — `syncthing@alice` becomes
/// `syncthing@alice.service`, not the `syncthing@.service` file behind it.
///
/// For the unit *file* behind that instance, see [`unit_file_name`].
pub fn qualified_unit_name(unit: &str) -> String {
    if UNIT_TYPE_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
    {
        unit.to_string()
    } else {
        format!("{unit}.service")
    }
}

/// The unit *file* a declared name resolves to, which is what
/// `systemctl list-unit-files` answers for: the same suffix rule, and then a
/// template instance collapses to its template, because `list-unit-files`
/// holds no entry for the instance — `syncthing@alice` is
/// `syncthing@.service`.
///
/// Appending `.service` unconditionally instead read `bichon-archive.timer` as
/// `bichon-archive.timer.service` and failed the restore preflight (#619).
///
/// For the loaded unit rather than the file, see [`qualified_unit_name`].
pub fn unit_file_name(unit: &str) -> String {
    let (name, suffix) = UNIT_TYPE_SUFFIXES
        .iter()
        .find_map(|suffix| unit.strip_suffix(suffix).map(|name| (name, *suffix)))
        .unwrap_or((unit, ".service"));
    match name.split_once('@') {
        Some((template, _)) => format!("{template}@{suffix}"),
        None => format!("{name}{suffix}"),
    }
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
    /// The App's Path Attestation: a command whose stdout lines are the paths
    /// the App itself reports its data lives at, checked against `paths`
    /// before `backup create` touches anything (ADR-0033).
    ///
    /// For an App that owns its data location in its own store — grimmory
    /// keeps the library root in a MariaDB row its UI writes — the role's
    /// declaration is a note that can quietly stop matching. Every line the
    /// command returns must sit within a declared path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attests: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<DbRecipe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_restore_command: Option<String>,
    /// What a human still has to do once the restore has landed — the App's
    /// own note, rendered after a cross-host restore and nowhere else (#671).
    /// Recipes without one render nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_advice: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parameters: HashMap<String, BackupParameter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbRecipe {
    pub name: String,
    pub dump_path: String,
    #[serde(default, skip_serializing_if = "DbEngine::is_postgres")]
    pub engine: DbEngine,
}

/// The database server a Recipe's `db:` block dumps from and restores into.
/// Defaults to postgres, the only engine the executor spoke before MariaDB
/// apps (grimmory, yourls) needed dumps too (#611).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbEngine {
    #[default]
    Postgres,
    Mariadb,
}

impl DbEngine {
    fn is_postgres(&self) -> bool {
        *self == Self::Postgres
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupParameter {
    #[serde(default)]
    pub default: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adds_paths: Vec<String>,
}

const ADMIN_USER_PLACEHOLDER: &str = "{admin_user}";

impl BackupRecipe {
    /// Substitute `{admin_user}` with the Host's user in every string field,
    /// so a Recipe can name per-user units and home paths (e.g. syncthing's
    /// `syncthing@<user>`) while staying pure data (ADR-0023).
    pub fn resolve(self, admin_user: &str) -> Self {
        let sub = |s: String| s.replace(ADMIN_USER_PLACEHOLDER, admin_user);
        Self {
            systemd_services: self.systemd_services.into_iter().map(sub).collect(),
            paths: self.paths.into_iter().map(sub).collect(),
            attests: self.attests.map(sub),
            owner: self.owner.map(|(user, group)| (sub(user), sub(group))),
            db: self.db.map(|db| DbRecipe {
                name: sub(db.name),
                dump_path: sub(db.dump_path),
                engine: db.engine,
            }),
            post_restore_command: self.post_restore_command.map(sub),
            restore_advice: self.restore_advice.map(sub),
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
    /// The units this App owns, as `systemctl` addresses them: names
    /// qualified, `{admin_user}` substituted, scope made explicit.
    pub fn owned_units(&self, admin_user: &str) -> Vec<OwnedUnit> {
        self.units
            .iter()
            .map(|decl| {
                let (name, scope) = match decl {
                    UnitDecl::Name(name) => (name.as_str(), UnitScope::System),
                    UnitDecl::Scoped { name, scope } => (name.as_str(), *scope),
                };
                OwnedUnit {
                    name: qualified_unit_name(&name.replace(ADMIN_USER_PLACEHOLDER, admin_user)),
                    scope,
                }
            })
            .collect()
    }

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

    fn playbook_files() -> Vec<std::path::PathBuf> {
        std::fs::read_dir(playbooks_dir())
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
            .collect()
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
        assert!(!meta.required_keys.contains(&"hostname".to_string()));
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
    }

    /// `tailscale_authkey` was infrastructure's third `required_key` until
    /// #768. It is minted per run by the CLI now, so demanding it of config
    /// would fail runs the CLI is about to satisfy — the invariant
    /// `tests/injected_keys.rs` holds over every Meta, asserted here on the
    /// one Meta that had it.
    #[test]
    fn test_infrastructure_meta_does_not_demand_the_injected_authkey() {
        let meta = load_meta("infrastructure");
        assert!(
            !meta
                .required_keys
                .contains(&crate::commands::headscale::INJECTED_AUTHKEY.to_string()),
            "{:?}",
            meta.required_keys
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
        assert_eq!(
            backup.systemd_services,
            vec!["bichon-archive.timer", "bichon"]
        );
        assert_eq!(
            backup.paths,
            vec!["/var/lib/bichon-archive", "/opt/bichon/data"]
        );
    }

    #[test]
    fn test_freshrss_meta_backup_recipe() {
        let backup = load_meta("freshrss").backup.unwrap();
        assert_eq!(backup.systemd_services, vec!["freshrss"]);
        assert_eq!(
            backup.paths,
            vec!["/var/lib/freshrss", "/opt/freshrss/data"]
        );
        assert!(
            backup
                .restore_advice
                .as_deref()
                .unwrap()
                .contains("verify feeds update"),
            "freshrss owns its own post-restore note (#671)"
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
    fn test_immich_meta_declares_no_backup_recipe() {
        let meta = load_meta("immich");
        assert!(!meta.tailnet_only);
        assert_eq!(meta.subdomain.as_deref(), Some("photos"));
        assert!(
            meta.backup.is_none(),
            "immich offsite backup runs on the Host (#558); a recipe here would \
             put a nightly multi-GiB stopped-unit pull into `backup sync`'s \
             default app set"
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
    fn test_syncthing_meta_backup_recipe() {
        let meta = load_meta("syncthing");
        assert!(meta.required_keys.is_empty());
        assert!(meta.version.is_none());
        let backup = meta
            .backup
            .expect("syncthing.meta.yml should declare backup");
        assert_eq!(backup.systemd_services, vec!["syncthing@{admin_user}"]);
        assert_eq!(
            backup.paths,
            vec![
                "/home/{admin_user}/.local/state/syncthing/config.xml",
                "/home/{admin_user}/.local/state/syncthing/cert.pem",
                "/home/{admin_user}/.local/state/syncthing/key.pem",
            ]
        );
        assert_eq!(
            backup.owner,
            Some(("{admin_user}".to_string(), "{admin_user}".to_string()))
        );
        assert!(backup.db.is_none());
        assert!(backup.post_restore_command.is_none());
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
        assert!(
            backup.restore_advice.as_deref().unwrap().contains("rescan"),
            "navidrome owns its own post-restore note (#671)"
        );
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
        let db = backup.db.expect("yourls declares db");
        assert_eq!(db.name, "yourls");
        assert_eq!(db.engine, DbEngine::Mariadb);
        assert_eq!(db.dump_path, "/tmp/yourls_db.dump");
    }

    #[test]
    fn test_grimmory_meta_backup_recipe() {
        let backup = load_meta("grimmory")
            .backup
            .expect("grimmory.meta.yml should declare backup");
        assert_eq!(backup.systemd_services, vec!["grimmory"]);
        assert_eq!(backup.paths, vec!["/srv/grimmory", "/srv/books"]);
        assert_eq!(
            backup.attests.as_deref(),
            Some("sudo mariadb -N -B -e 'select path from library_path' grimmory"),
            "grimmory owns its library root in its own database, so the Recipe verifies the \
             declaration instead of trusting it (ADR-0033)"
        );
        assert_eq!(
            backup.owner,
            Some(("grimmory".to_string(), "grimmory".to_string()))
        );
        let db = backup.db.expect("grimmory declares db");
        assert_eq!(db.name, "grimmory");
        assert_eq!(db.engine, DbEngine::Mariadb);
        assert_eq!(db.dump_path, "/tmp/grimmory_db.dump");
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
        assert_eq!(db.engine, DbEngine::Postgres);
        assert_eq!(db.dump_path, "/tmp/paperless_db.dump");
        let cmd = backup
            .post_restore_command
            .expect("paperless declares post_restore_command");
        assert!(cmd.contains("manage.py migrate"));

        let install_path = role_default("paperless", "paperless_install_path");
        let inside = cmd
            .strip_prefix("sudo -u paperless bash -c '")
            .and_then(|rest| rest.strip_suffix('\''))
            .unwrap_or_else(|| {
                panic!("the whole command must sit inside the sudo boundary (#608): {cmd}")
            });
        assert!(inside.contains(&format!("cd {install_path}/src")));
        assert!(inside.contains(&format!(
            "PAPERLESS_CONFIGURATION_PATH={install_path}/paperless.conf"
        )));
        assert!(inside.contains(&format!("{install_path}/venv/bin/python3")));
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
        let playbook_files = playbook_files();

        assert!(
            !playbook_files.is_empty(),
            "No playbook files found in playbooks dir"
        );

        for playbook in &playbook_files {
            let stem = playbook.file_stem().and_then(|s| s.to_str()).unwrap();
            let meta_path = playbooks_dir().join(format!("{stem}.meta.yml"));
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
            units: Vec::new(),
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
            "immich_version",
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
        assert!(backup.restore_advice.is_none());
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
  post_restore_command: "sudo -u paperless bash -c 'cd /opt/paperless/src && ./manage.py migrate'"
  restore_advice: "confirm the consumer picked the inbox back up"
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
        assert_eq!(
            db.engine,
            DbEngine::Postgres,
            "a db block without engine must keep dumping via pg_dump"
        );
        assert!(
            backup
                .post_restore_command
                .as_deref()
                .unwrap()
                .contains("manage.py migrate")
        );
        assert_eq!(
            backup.restore_advice.as_deref(),
            Some("confirm the consumer picked the inbox back up")
        );
    }

    #[test]
    fn test_db_recipe_with_mariadb_engine_parses() {
        let yaml = r#"
required_keys: []
backup:
  systemd_services: [grimmory]
  paths: [/srv/grimmory]
  db: { name: grimmory, engine: mariadb, dump_path: /tmp/grimmory_db.dump }
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        let db = meta.backup.unwrap().db.unwrap();
        assert_eq!(db.engine, DbEngine::Mariadb);
    }

    #[test]
    fn test_db_recipe_rejects_unknown_engine() {
        let yaml = r#"
required_keys: []
backup:
  db: { name: app, engine: sqlite, dump_path: /tmp/app.dump }
"#;
        let result: Result<PlaybookMeta, _> = serde_yaml::from_str(yaml);
        assert!(
            result.is_err(),
            "unknown engines must fail parse, not fall back"
        );
    }

    #[test]
    fn test_db_recipe_default_engine_round_trips_without_engine_key() {
        let recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec![],
            owner: None,
            db: Some(DbRecipe {
                name: "paperless".to_string(),
                dump_path: "/tmp/paperless_db.dump".to_string(),
                engine: DbEngine::Postgres,
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        };
        let yaml = serde_yaml::to_string(&recipe).unwrap();
        assert!(
            !yaml.contains("engine"),
            "default engine must not clutter serialized recipes: {yaml}"
        );
        let reparsed: BackupRecipe = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(reparsed, recipe);
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
            attests: None,
            restore_advice: None,
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
            attests: None,
            restore_advice: None,
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
            attests: None,
            restore_advice: None,
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
            attests: Some("echo /home/{admin_user}/Sync".to_string()),
            owner: Some(("{admin_user}".to_string(), "{admin_user}".to_string())),
            db: None,
            post_restore_command: Some("chown {admin_user} /tmp/x".to_string()),
            parameters,
            restore_advice: Some("check /home/{admin_user}/Sync came back".to_string()),
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
        assert_eq!(resolved.attests.as_deref(), Some("echo /home/alice/Sync"));
        assert_eq!(
            resolved.restore_advice.as_deref(),
            Some("check /home/alice/Sync came back")
        );
        assert_eq!(
            resolved.parameters.get("include_extra").unwrap().adds_paths,
            vec!["/home/alice/extra"]
        );
    }

    #[test]
    fn test_resolve_leaves_recipe_without_placeholders_unchanged() {
        let recipe = BackupRecipe {
            systemd_services: vec!["grimmory".to_string()],
            paths: vec!["/srv/grimmory".to_string()],
            owner: Some(("grimmory".to_string(), "grimmory".to_string())),
            db: Some(DbRecipe {
                name: "grimmory".to_string(),
                dump_path: "/tmp/grimmory_db.dump".to_string(),
                engine: DbEngine::Mariadb,
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
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
    fn test_meta_units_parse_bare_and_scoped_forms() {
        let yaml = r#"
required_keys: []
units:
  - liquidsoap
  - bichon-archive.timer
  - { name: hermes-gateway, scope: user }
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            meta.units,
            vec![
                UnitDecl::Name("liquidsoap".to_string()),
                UnitDecl::Name("bichon-archive.timer".to_string()),
                UnitDecl::Scoped {
                    name: "hermes-gateway".to_string(),
                    scope: UnitScope::User,
                },
            ]
        );
    }

    #[test]
    fn test_meta_without_units_parses_to_empty() {
        let yaml = "required_keys: []\n";
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert!(meta.units.is_empty());
    }

    #[test]
    fn test_owned_units_qualifies_resolves_and_scopes() {
        let yaml = r#"
required_keys: []
units:
  - "syncthing@{admin_user}"
  - colporteur.timer
  - { name: hermes-gateway, scope: user }
"#;
        let meta: PlaybookMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            meta.owned_units("alice"),
            vec![
                OwnedUnit {
                    name: "syncthing@alice.service".to_string(),
                    scope: UnitScope::System,
                },
                OwnedUnit {
                    name: "colporteur.timer".to_string(),
                    scope: UnitScope::System,
                },
                OwnedUnit {
                    name: "hermes-gateway.service".to_string(),
                    scope: UnitScope::User,
                },
            ]
        );
    }

    /// systemd's unit types are not something this repo can compute, so the
    /// closed set is declared here off `systemd.unit(5)` and matched against
    /// the production const by equality in both directions — the declared
    /// regime of ADR-0028, the shape `PURGED_PACKAGES` already has.
    ///
    /// Not the mirror ADR-0046 deletes: that one restated *this crate's* table
    /// inside a fence, so the fence and production could disagree. This
    /// restates *systemd's*, which no code here can read.
    ///
    /// It exists because the deleted mirrors were an accidental witness. Two
    /// fences each carried a copy, so truncating this list failed their drift
    /// check. Importing the const (#667) removed the drift — and the witness
    /// with it: a type dropped from the list below would shrink the qualifier,
    /// both fences' suffix tests, and both fences' domains, all green. That is
    /// #653's failure mode exactly, which is the one this file must not host.
    #[test]
    fn test_the_unit_type_set_is_every_type_systemd_defines() {
        const SYSTEMD_UNIT_TYPES: &[&str] = &[
            ".automount",
            ".device",
            ".mount",
            ".path",
            ".scope",
            ".service",
            ".slice",
            ".socket",
            ".swap",
            ".target",
            ".timer",
        ];
        let declared: std::collections::BTreeSet<&str> =
            UNIT_TYPE_SUFFIXES.iter().copied().collect();
        let systemd: std::collections::BTreeSet<&str> =
            SYSTEMD_UNIT_TYPES.iter().copied().collect();

        assert_eq!(
            systemd.difference(&declared).collect::<Vec<_>>(),
            Vec::<&&str>::new(),
            "UNIT_TYPE_SUFFIXES is missing unit types systemd defines; every \
             one is a name `qualified_unit_name` mis-suffixes and a removal the \
             fleet fences cannot see"
        );
        assert_eq!(
            declared.difference(&systemd).collect::<Vec<_>>(),
            Vec::<&&str>::new(),
            "UNIT_TYPE_SUFFIXES declares unit types systemd does not; a name \
             ending in one would be kept unqualified and address nothing"
        );
    }

    #[test]
    fn test_qualified_unit_name_keeps_explicit_types_and_appends_service() {
        assert_eq!(qualified_unit_name("gokapi"), "gokapi.service");
        assert_eq!(
            qualified_unit_name("bichon-archive.timer"),
            "bichon-archive.timer"
        );
        assert_eq!(qualified_unit_name("cockpit.socket"), "cockpit.socket");
        assert_eq!(
            qualified_unit_name("syncthing@alice"),
            "syncthing@alice.service"
        );
    }

    #[test]
    fn unit_file_name_appends_service_suffix() {
        assert_eq!(unit_file_name("freshrss"), "freshrss.service");
    }

    #[test]
    fn unit_file_name_maps_template_instance_to_template_file() {
        assert_eq!(unit_file_name("syncthing@alice"), "syncthing@.service");
    }

    #[test]
    fn unit_file_name_keeps_an_explicit_unit_type_suffix() {
        assert_eq!(
            unit_file_name("bichon-archive.timer"),
            "bichon-archive.timer"
        );
        assert_eq!(unit_file_name("bichon.service"), "bichon.service");
    }

    #[test]
    fn unit_file_name_appends_service_to_a_dotted_name_that_is_not_a_unit_type() {
        assert_eq!(unit_file_name("foo.bar"), "foo.bar.service");
    }

    #[test]
    fn unit_file_name_maps_a_suffixed_template_instance_to_its_template_file() {
        assert_eq!(unit_file_name("backup@daily.timer"), "backup@.timer");
    }

    #[test]
    fn test_radio_meta_declares_its_packaged_unit_too() {
        let meta = load_meta("radio");
        assert_eq!(
            meta.owned_units("alice"),
            vec![
                OwnedUnit {
                    name: "liquidsoap.service".to_string(),
                    scope: UnitScope::System,
                },
                OwnedUnit {
                    name: "icecast2.service".to_string(),
                    scope: UnitScope::System,
                },
            ],
            "icecast2 is apt-packaged — the role only drops in over it — and \
             still the App's to answer for (#644)"
        );
    }

    #[test]
    fn test_hermes_meta_declares_its_gateway_as_a_user_unit() {
        let meta = load_meta("hermes");
        assert_eq!(
            meta.owned_units("alice"),
            vec![OwnedUnit {
                name: "hermes-gateway.service".to_string(),
                scope: UnitScope::User,
            }],
            "a user unit is invisible to `systemctl show` without --user, so \
             the declaration must carry the scope"
        );
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

    /// The shell name a token assigns to, if the token is a `NAME=value`
    /// environment prefix.
    fn env_assignment(token: &str) -> Option<&str> {
        let (name, _) = token.split_once('=')?;
        let mut chars = name.chars();
        let first = chars.next()?;
        if !first.is_ascii_alphabetic() && first != '_' {
            return None;
        }
        chars
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            .then_some(name)
    }

    /// The command as the ssh user's own shell sees it: the `&&`/`||`/`;`/`|`
    /// separated segments it runs in sequence, each split into tokens. Quoted
    /// spans stay inside their token, because a `bash -c '…'` body is one
    /// argument to sudo — opaque to the outer shell, and already inside
    /// whatever boundary precedes it.
    fn shell_segments(cmd: &str) -> Vec<Vec<String>> {
        let chars: Vec<char> = cmd.chars().collect();
        let mut segments = vec![Vec::new()];
        let mut token = String::new();
        let mut quote: Option<char> = None;
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];
            i += 1;
            match quote {
                Some(open) if c == open => quote = None,
                Some(_) => token.push(c),
                None if c == '\'' || c == '"' => quote = Some(c),
                None if c.is_whitespace() || matches!(c, ';' | '&' | '|') => {
                    if !token.is_empty() {
                        segments
                            .last_mut()
                            .unwrap()
                            .push(std::mem::take(&mut token));
                    }
                    if matches!(c, ';' | '&' | '|') {
                        while i < chars.len() && matches!(chars[i], ';' | '&' | '|') {
                            i += 1;
                        }
                        segments.push(Vec::new());
                    }
                }
                None => token.push(c),
            }
        }
        if !token.is_empty() {
            segments.last_mut().unwrap().push(token);
        }
        segments
    }

    /// Every token the ssh user runs itself: per segment, the tokens up to that
    /// segment's `sudo`. A segment naming no `sudo` is unprivileged end to end,
    /// so all of its tokens count.
    fn tokens_run_as_ssh_user(cmd: &str) -> Vec<String> {
        shell_segments(cmd)
            .into_iter()
            .flat_map(|segment| {
                segment
                    .into_iter()
                    .take_while(|token| token != "sudo" && !token.ends_with("/sudo"))
            })
            .collect()
    }

    /// Everything a `post_restore_command` leaves to the left of a `sudo` runs
    /// as the ssh user, before the privilege boundary exists: a `cd` into an
    /// App's `0750` tree is denied on directory traversal, and an `ENV=value`
    /// prefix is wiped by sudo's `env_reset` before the App ever reads it. Both
    /// fired on the first real cross-host paperless restore (#608), where every
    /// byte landed and only `manage.py migrate` never ran. Recipes are pure
    /// data, so the fence is this lint: put the whole command inside the sudo
    /// boundary — `sudo -u <app> bash -c '<cd && ENV=… cmd>'`, where the quoted
    /// body is one argument and out of the outer shell's reach.
    #[test]
    fn test_post_restore_commands_keep_privileged_work_inside_the_sudo_boundary() {
        for (app, meta) in load_all_metas(&playbooks_dir()).unwrap() {
            let Some(cmd) = meta.backup.and_then(|backup| backup.post_restore_command) else {
                continue;
            };
            for token in tokens_run_as_ssh_user(&cmd) {
                assert_ne!(
                    token, "cd",
                    "{app}: post_restore_command runs `cd` as the ssh user — traversing a \
                     0750 App directory is denied. Wrap the whole command: \
                     sudo -u <app> bash -c '<cd … && …>'. Got: {cmd}"
                );
                assert_eq!(
                    env_assignment(&token),
                    None,
                    "{app}: post_restore_command sets {token} as the ssh user — env_reset \
                     strips it before the App reads it. Wrap the whole command: \
                     sudo -u <app> bash -c '<… ENV=value …>'. Got: {cmd}"
                );
            }
        }
    }

    /// A fence that silently stops fencing is worse than none, and this one
    /// carries real parsing: it must read the quoted `bash -c` body as one
    /// opaque argument while still seeing past a `sudo` that only covers the
    /// first segment.
    #[test]
    fn test_sudo_boundary_lint_reads_each_shell_segment_on_its_own() {
        let fixed = "sudo -u paperless bash -c 'cd /opt/paperless/src && PAPERLESS_CONF=/x \
                     /opt/paperless/venv/bin/python3 manage.py migrate --no-input'";
        assert_eq!(
            tokens_run_as_ssh_user(fixed),
            Vec::<String>::new(),
            "the quoted body is one argument to sudo, not the outer shell's work"
        );

        let incident = "cd /opt/paperless/src && PAPERLESS_CONF=/x sudo -u paperless python3 \
                        manage.py migrate";
        let unprivileged = tokens_run_as_ssh_user(incident);
        assert!(unprivileged.contains(&"cd".to_string()));
        assert!(unprivileged.iter().any(|t| env_assignment(t).is_some()));

        let escapes_a_first_segment_sudo = "sudo -u app true && cd /opt/app && FOO=1 ./x";
        let unprivileged = tokens_run_as_ssh_user(escapes_a_first_segment_sudo);
        assert!(
            unprivileged.contains(&"cd".to_string()),
            "a sudo covering only the first segment leaves the rest unprivileged"
        );
        assert_eq!(
            unprivileged.iter().filter_map(|t| env_assignment(t)).next(),
            Some("FOO")
        );

        assert_eq!(
            tokens_run_as_ssh_user("chown -R paperless /opt/paperless/media"),
            vec!["chown", "-R", "paperless", "/opt/paperless/media"],
            "a Recipe naming no sudo is unprivileged end to end"
        );
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
    fn test_no_playbook_targets_localhost() {
        for path in playbook_files() {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let raw = std::fs::read_to_string(&path).unwrap();
            let plays: Vec<serde_yaml::Value> = serde_yaml::from_str(&raw)
                .unwrap_or_else(|e| panic!("Failed to parse {name}: {e}"));
            for play in &plays {
                if play.get("import_playbook").is_some() {
                    continue;
                }
                let patterns: Vec<&str> = match play.get("hosts") {
                    Some(serde_yaml::Value::String(s)) => {
                        s.split([',', ':']).map(str::trim).collect()
                    }
                    Some(serde_yaml::Value::Sequence(seq)) => {
                        seq.iter().filter_map(|v| v.as_str()).collect()
                    }
                    other => panic!(
                        "{name}: expected a scalar or list hosts target, got {other:?} — \
                         update this test for the new play shape"
                    ),
                };
                assert!(
                    !patterns.contains(&"localhost"),
                    "{name}: `run_playbook` always passes --limit, which excludes \
                     implicit localhost — a localhost play never executes"
                );
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
