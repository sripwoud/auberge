use crate::ansible_assets::AnsibleAssets;
use crate::config::{Config, Preflight};
use crate::key_registry::KeyRegistry;
use crate::playbook_meta::PlaybookMeta;
use crate::services::dependency_resolver::parse_playbook_roles;
use eyre::Result;
use std::path::{Path, PathBuf};

const META_SUFFIX: &str = ".meta.yml";
const REGISTRY_FILE: &str = "keys.yml";
const PLAYBOOKS_DIR: &str = "playbooks";

/// The `required_keys` one Playbook Meta declares, empty when the App has no
/// Meta. Every name is checked against the Key Registry here, so a typo fails
/// where the declaration is read rather than mid-play.
fn declared_keys(playbooks_dir: &Path, stem: &str, registry: &KeyRegistry) -> Result<Vec<String>> {
    let path = playbooks_dir.join(format!("{stem}{META_SUFFIX}"));
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let keys = PlaybookMeta::load(&path)?.required_keys;
    for key in &keys {
        if registry.get(key).is_none() {
            eyre::bail!(
                "{} declares required key '{key}', which is absent from the Key Registry",
                path.display()
            );
        }
    }
    Ok(keys)
}

/// The roster roles a tag selection resolves to: a role is selected when one of
/// its declared tags was named, or when the tag is the role's own name.
fn selected_roles(playbook_path: &PathBuf, tags: &[String]) -> Result<Vec<String>> {
    if !playbook_path.is_file() {
        return Ok(Vec::new());
    }
    Ok(parse_playbook_roles(playbook_path)?
        .into_iter()
        .filter(|(role, role_tags)| {
            tags.iter()
                .any(|tag| tag == role || role_tags.contains(tag))
        })
        .map(|(role, _)| role)
        .collect())
}

/// The effective required config keys for one Playbook run: the Playbook's own
/// Meta declarations unioned with the Metas of the roles the selected tags
/// resolve to.
///
/// An untagged run resolves no roles: the roster's `when:`-guarded roles do not
/// run on every Host, so unioning the whole roster would demand keys the run
/// never reads.
pub fn required_keys_for(
    ansible_dir: &Path,
    playbook: &str,
    tags: Option<&[String]>,
) -> Result<Vec<String>> {
    let playbooks_dir = ansible_dir.join(PLAYBOOKS_DIR);
    let registry = KeyRegistry::load(&ansible_dir.join(REGISTRY_FILE))?;
    let stem = playbook
        .strip_suffix(".yml")
        .or_else(|| playbook.strip_suffix(".yaml"))
        .unwrap_or(playbook);

    let mut sources = vec![stem.to_string()];
    sources.extend(selected_roles(
        &playbooks_dir.join(playbook),
        tags.unwrap_or_default(),
    )?);

    let mut keys: Vec<String> = Vec::new();
    for source in sources {
        for key in declared_keys(&playbooks_dir, &source, &registry)? {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    Ok(keys)
}

/// Build a [`Preflight`] for `playbook`, validating every key the Playbook
/// Metas declare for this run. The only path to a `Preflight`, and so the only
/// path to an Ansible run.
pub fn preflight_for(
    config: &Config,
    playbook: &str,
    tags: Option<&[String]>,
) -> Result<Preflight> {
    let assets = AnsibleAssets::prepare()?;
    config.preflight_with_keys(required_keys_for(assets.ansible_dir(), playbook, tags)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn repo_ansible_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible")
    }

    fn playbooks_dir() -> PathBuf {
        repo_ansible_dir().join(PLAYBOOKS_DIR)
    }

    fn registry() -> KeyRegistry {
        KeyRegistry::load(&repo_ansible_dir().join(REGISTRY_FILE)).unwrap()
    }

    /// An ansible dir holding a Key Registry of `keys` plus the given
    /// `<name>` → file-body pairs under `playbooks/`.
    fn fixture_ansible_dir(keys: &[&str], files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = String::from("keys:\n");
        for key in keys {
            registry.push_str(&format!("  {key}:\n    secret: false\n    doc: test key\n"));
        }
        std::fs::write(dir.path().join(REGISTRY_FILE), registry).unwrap();
        let playbooks = dir.path().join(PLAYBOOKS_DIR);
        std::fs::create_dir_all(&playbooks).unwrap();
        for (name, body) in files {
            std::fs::write(playbooks.join(name), body).unwrap();
        }
        dir
    }

    fn roster_roles() -> Vec<String> {
        parse_playbook_roles(&playbooks_dir().join("apps.yml"))
            .unwrap()
            .into_iter()
            .map(|(role, _)| role)
            .collect()
    }

    // ── union semantics ───────────────────────────────────────────────────────

    #[test]
    fn test_untagged_run_takes_only_the_playbooks_own_meta() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "app_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("app.meta.yml", "required_keys: [app_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: app\n      tags: [apps, app]\n",
                ),
            ],
        );
        let keys = required_keys_for(dir.path(), "apps.yml", None).unwrap();
        assert_eq!(keys, vec!["admin_user_name".to_string()]);
    }

    #[test]
    fn test_tagged_run_unions_the_selected_roles_meta() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "app_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("app.meta.yml", "required_keys: [app_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: app\n      tags: [apps, app]\n",
                ),
            ],
        );
        let tags = vec!["app".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(
            keys,
            vec!["admin_user_name".to_string(), "app_token".to_string()]
        );
    }

    #[test]
    fn test_category_tag_unions_every_role_carrying_it() {
        let dir = fixture_ansible_dir(
            &["base", "one_key", "two_key", "three_key"],
            &[
                ("apps.meta.yml", "required_keys: [base]\n"),
                ("one.meta.yml", "required_keys: [one_key]\n"),
                ("two.meta.yml", "required_keys: [two_key]\n"),
                ("three.meta.yml", "required_keys: [three_key]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: one\n      tags: [apps, media, one]\n    - role: two\n      tags: [apps, media, two]\n    - role: three\n      tags: [apps, web, three]\n",
                ),
            ],
        );
        let tags = vec!["media".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(
            keys,
            vec![
                "base".to_string(),
                "one_key".to_string(),
                "two_key".to_string()
            ]
        );
        assert!(!keys.contains(&"three_key".to_string()));
    }

    #[test]
    fn test_union_deduplicates_keys_declared_twice() {
        let dir = fixture_ansible_dir(
            &["domain"],
            &[
                ("apps.meta.yml", "required_keys: [domain]\n"),
                ("app.meta.yml", "required_keys: [domain]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: app\n      tags: [apps, app]\n",
                ),
            ],
        );
        let tags = vec!["app".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(keys, vec!["domain".to_string()]);
    }

    #[test]
    fn test_role_without_a_meta_contributes_nothing() {
        let dir = fixture_ansible_dir(
            &["base"],
            &[
                ("apps.meta.yml", "required_keys: [base]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: metaless\n      tags: [apps, metaless]\n",
                ),
            ],
        );
        let tags = vec!["metaless".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(keys, vec!["base".to_string()]);
    }

    #[test]
    fn test_standalone_playbook_reads_its_own_meta() {
        let dir = fixture_ansible_dir(
            &["solo_token"],
            &[("solo.meta.yml", "required_keys: [solo_token]\n")],
        );
        let keys = required_keys_for(dir.path(), "solo.yml", None).unwrap();
        assert_eq!(keys, vec!["solo_token".to_string()]);
    }

    #[test]
    fn test_playbook_without_a_meta_requires_nothing() {
        let dir = fixture_ansible_dir(&["unused"], &[]);
        let keys = required_keys_for(dir.path(), "ghost.yml", None).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_unknown_key_name_in_a_meta_is_rejected() {
        let dir = fixture_ansible_dir(
            &["known"],
            &[("solo.meta.yml", "required_keys: [known, typoed_key]\n")],
        );
        let err = required_keys_for(dir.path(), "solo.yml", None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("typoed_key"), "{msg}");
        assert!(msg.contains("Key Registry"), "{msg}");
    }

    // ── the repo's own Metas ──────────────────────────────────────────────────

    #[test]
    fn test_every_declared_required_key_exists_in_the_key_registry() {
        let registry = registry();
        let mut unknown = Vec::new();
        for (app, meta) in crate::playbook_meta::load_all_metas(&playbooks_dir()).unwrap() {
            for key in meta.required_keys {
                if registry.get(&key).is_none() {
                    unknown.push(format!("{app}: {key}"));
                }
            }
        }
        assert!(
            unknown.is_empty(),
            "Metas declare required keys absent from the Key Registry: {}",
            unknown.join(", ")
        );
    }

    #[test]
    fn test_every_roster_role_has_a_playbook_meta() {
        let missing: Vec<String> = roster_roles()
            .into_iter()
            .filter(|role| {
                !playbooks_dir()
                    .join(format!("{role}{META_SUFFIX}"))
                    .is_file()
            })
            .collect();
        assert!(
            missing.is_empty(),
            "apps.yml roster roles without a Playbook Meta: {}",
            missing.join(", ")
        );
    }

    #[test]
    fn test_every_playbook_has_a_playbook_meta() {
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(playbooks_dir()).unwrap() {
            let path = entry.unwrap().path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.ends_with(META_SUFFIX) || !name.ends_with(".yml") {
                continue;
            }
            let stem = name.strip_suffix(".yml").unwrap();
            if !playbooks_dir()
                .join(format!("{stem}{META_SUFFIX}"))
                .is_file()
            {
                missing.push(name.to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "playbooks without a Playbook Meta (their runs would validate nothing): {}",
            missing.join(", ")
        );
    }

    #[test]
    fn test_repo_apps_playbook_resolves_the_base_keys_untagged() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("admin_user_name"), "{keys:?}");
        assert!(set.contains("domain"), "{keys:?}");
        assert!(set.contains("cloudflare_dns_api_token"), "{keys:?}");
    }

    #[test]
    fn test_repo_bootstrap_keys_come_from_its_meta() {
        let keys = required_keys_for(&repo_ansible_dir(), "bootstrap.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert_eq!(
            set,
            HashSet::from(["admin_user_name", "ssh_port", "hostname"])
        );
    }

    /// Every key the audit in ADR-0044 found an App role to hard-require: an
    /// in-role `assert` names it, or it is referenced unguarded with no default
    /// in the role, group_vars, or anywhere else. Enforced at Preflight now,
    /// where before the run failed mid-play.
    const APP_SPECIFIC_KEYS: &[(&str, &[&str])] = &[
        (
            "baikal",
            &[
                "admin_user_email",
                "baikal_admin_password",
                "baikal_busy_feed_token",
                "baikal_subdomain",
            ],
        ),
        (
            "bichon",
            &["bichon_api_token", "bichon_encryption_password"],
        ),
        (
            "colporteur",
            &["colporteur_feeds_password", "colporteur_subdomain"],
        ),
        ("freshrss", &["freshrss_subdomain"]),
        (
            "gokapi",
            &[
                "gokapi_admin_password",
                "gokapi_admin_user",
                "gokapi_subdomain",
            ],
        ),
        (
            "grimmory",
            &[
                "grimmory_admin_password",
                "grimmory_admin_user",
                "grimmory_db_password",
                "grimmory_subdomain",
            ],
        ),
        (
            "immich",
            &[
                "immich_b2_application_key",
                "immich_b2_key_id",
                "immich_db_password",
                "immich_restic_password",
                "immich_restic_repository",
                "immich_subdomain",
            ],
        ),
        ("navidrome", &["navidrome_subdomain"]),
        (
            "paperless",
            &[
                "admin_user_email",
                "paperless_admin_password",
                "paperless_admin_user",
                "paperless_db_password",
                "paperless_secret_key",
            ],
        ),
        ("radio", &["radio_listener_password", "radio_subdomain"]),
        ("tgtg", &["tgtg_telegram_bot_token"]),
        (
            "yourls",
            &[
                "yourls_admin_password",
                "yourls_admin_user",
                "yourls_cookiekey",
                "yourls_db_password",
                "yourls_subdomain",
            ],
        ),
    ];

    #[test]
    fn test_app_tag_resolves_every_key_its_role_requires() {
        let mut gaps = Vec::new();
        for (app, expected) in APP_SPECIFIC_KEYS {
            let tags = vec![(*app).to_string()];
            let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", Some(&tags)).unwrap();
            for key in *expected {
                if !keys.iter().any(|k| k == key) {
                    gaps.push(format!("{app}: {key}"));
                }
            }
        }
        assert!(
            gaps.is_empty(),
            "app tags that do not resolve a key their role requires: {}",
            gaps.join(", ")
        );
    }

    #[test]
    fn test_app_tag_still_resolves_the_shared_base_keys() {
        let tags = vec!["colporteur".to_string()];
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", Some(&tags)).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("admin_user_name"), "{keys:?}");
        assert!(set.contains("domain"), "{keys:?}");
        assert!(set.contains("cloudflare_dns_api_token"), "{keys:?}");
    }

    #[test]
    fn test_category_tag_unions_the_apps_beneath_it() {
        let tags = vec!["media".to_string()];
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", Some(&tags)).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for key in [
            "radio_listener_password",
            "navidrome_subdomain",
            "immich_db_password",
        ] {
            assert!(set.contains(key), "media should resolve {key}: {keys:?}");
        }
        assert!(
            !set.contains("yourls_cookiekey"),
            "media must not resolve a web App's key: {keys:?}"
        );
    }

    /// An App that is also a standalone playbook cannot lean on
    /// `apps.meta.yml` for the shared base, so its own Meta carries them.
    #[test]
    fn test_standalone_app_playbook_resolves_its_own_base_keys() {
        for app in ["gokapi", "immich"] {
            let keys = required_keys_for(&repo_ansible_dir(), &format!("{app}.yml"), None).unwrap();
            let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
            assert!(set.contains("domain"), "{app}: {keys:?}");
            assert!(set.contains("cloudflare_dns_api_token"), "{app}: {keys:?}");
        }
    }

    #[test]
    fn test_repo_hardening_requires_nothing() {
        let keys = required_keys_for(&repo_ansible_dir(), "hardening.yml", None).unwrap();
        assert!(keys.is_empty(), "{keys:?}");
    }
}
