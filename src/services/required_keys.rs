use crate::config::{Config, Preflight};
use crate::key_registry::KeyRegistry;
use crate::playbook_meta::PlaybookMeta;
use crate::services::dependency_resolver::parse_roster;
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

/// The roster roles a run enters.
///
/// A tagged run selects a role when one of its declared tags was named, or when
/// the tag is the role's own name. An untagged run enters the whole roster, so
/// it selects every entry that is not `when:`-guarded — a guard turns on Host
/// facts no caller can evaluate before the play runs, and demanding its keys
/// would fail Hosts the role never touches.
fn selected_roles(playbook_path: &Path, tags: &[String]) -> Result<Vec<String>> {
    if !playbook_path.is_file() {
        return Ok(Vec::new());
    }
    Ok(parse_roster(playbook_path)?
        .into_iter()
        .filter(|role| match tags {
            [] => !role.guarded,
            tags => tags
                .iter()
                .any(|tag| *tag == role.name || role.tags.contains(tag)),
        })
        .map(|role| role.name)
        .collect())
}

/// The Playbook file for `stem`, whichever extension it carries.
fn roster_path(playbooks_dir: &Path, stem: &str) -> PathBuf {
    let yml = playbooks_dir.join(format!("{stem}.yml"));
    if yml.is_file() {
        return yml;
    }
    playbooks_dir.join(format!("{stem}.yaml"))
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
        &roster_path(&playbooks_dir, stem),
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
/// Metas declare for this run. Every production path to an Ansible run comes
/// through here, so the Metas are the only authority a deploy consults.
///
/// `ansible_dir` is the caller's already-prepared Assets Tree: a deploy
/// preflights every run in its plan, and preparing the tree per run would take
/// the extract-and-sweep lock once per playbook instead of once (ADR-0034).
pub fn preflight_for(
    config: &Config,
    ansible_dir: &Path,
    playbook: &str,
    tags: Option<&[String]>,
    host: &str,
) -> Result<Preflight> {
    let known: Vec<String> = crate::services::inventory::get_hosts(None, None)?
        .into_iter()
        .map(|h| h.name)
        .collect();
    assert_host_overrides_known(config, &known)?;
    config.preflight_with_keys(&required_keys_for(ansible_dir, playbook, tags)?, Some(host))
}

/// Every `[hosts.<name>]` table must name a Host the roster knows: a typoed
/// name is a fail-open — the run proceeds on the fleet-wide answers the table
/// meant to withdraw (ADR-0057).
pub(crate) fn assert_host_overrides_known(config: &Config, known: &[String]) -> Result<()> {
    for name in config.host_override_names() {
        if !known.contains(&name) {
            eyre::bail!(
                "[hosts.{name}] in config.toml names no known host (known: {})",
                known.join(", ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn repo_ansible_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible")
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

    // ── union semantics ───────────────────────────────────────────────────────

    #[test]
    fn test_untagged_run_unions_every_unguarded_roster_role() {
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
        assert_eq!(
            keys,
            vec!["admin_user_name".to_string(), "app_token".to_string()]
        );
    }

    #[test]
    fn test_untagged_run_skips_a_when_guarded_role() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "guarded_token", "plain_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("guarded.meta.yml", "required_keys: [guarded_token]\n"),
                ("plain.meta.yml", "required_keys: [plain_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: guarded\n      tags: [apps, guarded]\n      when: \"'x' in group_names\"\n    - role: plain\n      tags: [apps, plain]\n",
                ),
            ],
        );
        let keys = required_keys_for(dir.path(), "apps.yml", None).unwrap();
        assert_eq!(
            keys,
            vec!["admin_user_name".to_string(), "plain_token".to_string()]
        );
    }

    /// Naming the guarded role's tag is the operator asserting the role runs,
    /// so its keys are demanded — only the untagged sweep skips it.
    #[test]
    fn test_naming_a_guarded_roles_tag_still_demands_its_keys() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "guarded_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("guarded.meta.yml", "required_keys: [guarded_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: guarded\n      tags: [apps, guarded]\n      when: \"'x' in group_names\"\n",
                ),
            ],
        );
        let tags = vec!["guarded".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert!(keys.contains(&"guarded_token".to_string()), "{keys:?}");
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
    fn test_repo_apps_playbook_resolves_the_base_keys_untagged() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("admin_user_name"), "{keys:?}");
        assert!(set.contains("domain"), "{keys:?}");
        assert!(set.contains("cloudflare_dns_api_token"), "{keys:?}");
    }

    /// The run enters every unguarded App, so it demands their keys too — this
    /// is the case the drift left failing mid-play.
    #[test]
    fn test_repo_untagged_apps_run_demands_the_app_keys() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for key in [
            "yourls_cookiekey",
            "paperless_secret_key",
            "grimmory_db_password",
            "radio_listener_password",
            "immich_db_password",
        ] {
            assert!(
                set.contains(key),
                "untagged apps should demand {key}: {keys:?}"
            );
        }
    }

    /// `hermes` is the roster's one `when:`-guarded App: it runs only on Hosts
    /// in the hermes group, so an untagged run cannot demand its keys.
    #[test]
    fn test_repo_untagged_apps_run_skips_the_guarded_app() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        assert!(
            !keys.iter().any(|k| k == "hermes_llm_api_key"),
            "untagged apps must not demand a guarded App's keys: {keys:?}"
        );
    }

    /// `blocky` and `headscale` carry infrastructure's `when:` gates
    /// (ADR-0051, ADR-0057): whether they run is the target Host's answer, so
    /// an untagged run cannot demand their keys.
    #[test]
    fn test_repo_untagged_infrastructure_run_skips_the_guarded_roles() {
        let keys = required_keys_for(&repo_ansible_dir(), "infrastructure.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("tailscale_authkey"), "{keys:?}");
        for gate in ["blocky_subdomain", "headscale_subdomain"] {
            assert!(
                !set.contains(gate),
                "untagged infrastructure must not demand a guarded role's key: {keys:?}"
            );
        }
    }

    /// The gate reads `headscale_subdomain` from config alone (#710), so a run
    /// that names the tag without the key would skip every task it asked for.
    /// Naming the tag is the operator asserting the role runs, and then they
    /// are asked. Any selecting tag counts, category tags included — ADR-0045's
    /// selection rule, pinned here so a change to it is deliberate.
    #[test]
    fn test_repo_headscale_tag_demands_the_gate_key() {
        for tag in ["headscale", "vpn"] {
            let tags = vec![tag.to_string()];
            let keys =
                required_keys_for(&repo_ansible_dir(), "infrastructure.yml", Some(&tags)).unwrap();
            assert!(
                keys.iter().any(|k| k == "headscale_subdomain"),
                "-t {tag} must demand headscale_subdomain: {keys:?}"
            );
        }
    }

    #[test]
    fn test_repo_blocky_tag_demands_the_gate_key() {
        for tag in ["blocky", "dns"] {
            let tags = vec![tag.to_string()];
            let keys =
                required_keys_for(&repo_ansible_dir(), "infrastructure.yml", Some(&tags)).unwrap();
            assert!(
                keys.iter().any(|k| k == "blocky_subdomain"),
                "-t {tag} must demand blocky_subdomain: {keys:?}"
            );
        }
    }

    #[test]
    fn test_repo_bootstrap_keys_come_from_its_meta() {
        let keys = required_keys_for(&repo_ansible_dir(), "bootstrap.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert_eq!(set, HashSet::from(["admin_user_name", "ssh_port"]));
    }

    /// Every key the audit in ADR-0045 found an App role to hard-require: an
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
        ("calibre", &["calibre_subdomain"]),
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

    #[test]
    fn test_host_override_tables_must_name_known_hosts() {
        let config = Config::from_toml_str(
            r#"
            [hosts.agentbox]
            headscale_subdomain = ""
        "#,
        )
        .unwrap();
        let known = vec!["auberge".to_string(), "agent-box".to_string()];
        let err = assert_host_overrides_known(&config, &known).unwrap_err();
        assert!(err.to_string().contains("agentbox"), "{err}");

        let config = Config::from_toml_str(
            r#"
            [hosts.agent-box]
            headscale_subdomain = ""
        "#,
        )
        .unwrap();
        assert!(assert_host_overrides_known(&config, &known).is_ok());
    }
}
