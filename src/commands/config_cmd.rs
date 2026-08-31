use crate::ansible_assets::AnsibleAssets;
use crate::config::Config;
use crate::key_registry::KeyRegistry;
use crate::output;
use crate::prompt::{Choice, select_item};
use crate::services::required_keys::required_keys_for;
use clap::{Args, Subcommand};
use dialoguer::{Input, theme::ColorfulTheme};
use eyre::{Result, WrapErr};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum ConfigCommands {
    #[command(
        visible_alias = "i",
        about = "Print a config.toml scaffold derived from the Key Registry"
    )]
    Init(InitArgs),
    #[command(visible_alias = "s", about = "Set a config value")]
    Set {
        #[arg(help = "Key name (e.g. admin_user_name)")]
        key: Option<String>,
        #[arg(help = "Value to set")]
        value: Option<String>,
    },
    #[command(visible_alias = "g", about = "Get a config value")]
    Get {
        #[arg(help = "Key name")]
        key: Option<String>,
        #[arg(
            long,
            help = "Execute !-prefixed command references and print the resolved value (may print secrets)"
        )]
        resolved: bool,
    },
    #[command(
        visible_alias = "l",
        about = "List all config keys (sensitive values redacted)"
    )]
    List,
    #[command(visible_alias = "rm", about = "Remove a key from config")]
    Remove {
        #[arg(help = "Key name")]
        key: Option<String>,
    },
    #[command(visible_alias = "e", about = "Open config in $EDITOR")]
    Edit,
    #[command(visible_alias = "p", about = "Print config file path")]
    Path,
}

#[derive(Args)]
pub struct InitArgs {
    #[arg(
        long,
        value_delimiter = ',',
        help = "Comma-separated playbook names; emit only their union of required keys"
    )]
    pub playbooks: Vec<String>,
    #[arg(
        short = 'o',
        long,
        help = "Write scaffold to FILE (refuses to overwrite without --force)"
    )]
    pub output: Option<PathBuf>,
    #[arg(short = 'f', long, help = "Overwrite the output file if it exists")]
    pub force: bool,
}

fn key_choice(prompt: &str) -> Choice {
    Choice::new("config key")
        .with_prompt(prompt)
        .resolved_by("the key as an argument")
}

fn select_key(config: &Config, prompt: &str) -> Result<String> {
    let mut keys = config.keys();
    keys.retain(|k| k != crate::config::HOSTS_TABLE);
    if keys.is_empty() {
        eyre::bail!("No config keys found");
    }
    select_item(&keys, |s: &String| s.clone(), key_choice(prompt))
}

fn select_registry_key(registry: &KeyRegistry, prompt: &str) -> Result<String> {
    let keys = sorted_registry_keys(registry);
    if keys.is_empty() {
        eyre::bail!("Key Registry is empty");
    }
    let display = |k: &String| match registry.get(k) {
        Some(entry) if entry.secret => format!("{k} [secret]"),
        _ => k.clone(),
    };
    select_item(&keys, display, key_choice(prompt))
}

fn sorted_registry_keys(registry: &KeyRegistry) -> Vec<String> {
    let mut keys: Vec<String> = registry.iter().map(|(k, _)| k.clone()).collect();
    keys.sort();
    keys
}

fn resolve_key(key: Option<String>, config: &Config, prompt: &str) -> Result<String> {
    match key {
        Some(k) => Ok(k),
        None => select_key(config, prompt),
    }
}

pub fn run_config_init(args: InitArgs) -> Result<()> {
    let assets = AnsibleAssets::prepare()?;
    let registry = KeyRegistry::load(&assets.ansible_dir().join("keys.yml"))?;
    let scaffold = build_scaffold(&registry, &args.playbooks, assets.ansible_dir())?;

    match args.output {
        None => {
            print!("{scaffold}");
            Ok(())
        }
        Some(path) => write_scaffold(&path, &scaffold, args.force),
    }
}

/// Scaffold the keys `playbooks` require, resolved through the same seam a
/// deploy uses so `config init` and Preflight cannot disagree about what a
/// Playbook needs.
fn build_scaffold(
    registry: &KeyRegistry,
    playbooks: &[String],
    ansible_dir: &Path,
) -> Result<String> {
    if playbooks.is_empty() {
        return Ok(registry.scaffold());
    }
    let mut keys: HashSet<String> = HashSet::new();
    for playbook in playbooks {
        if !ansible_dir
            .join("playbooks")
            .join(format!("{playbook}.meta.yml"))
            .is_file()
        {
            eyre::bail!("Unknown playbook '{playbook}'");
        }
        keys.extend(required_keys_for(ansible_dir, playbook, None)?);
    }
    Ok(registry.scaffold_filtered(&keys))
}

fn write_scaffold(path: &Path, scaffold: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        eyre::bail!(
            "Refusing to overwrite {}; pass --force to override",
            path.display()
        );
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(path, scaffold).wrap_err_with(|| format!("Failed to write {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .wrap_err_with(|| format!("Failed to set permissions on {}", path.display()))?;
    output::success(&format!("Wrote scaffold to {}", path.display()));
    Ok(())
}

pub fn run_config_set(key: Option<String>, value: Option<String>) -> Result<()> {
    let mut config = Config::load()?;
    let key = match key {
        Some(k) => k,
        None => {
            let assets = AnsibleAssets::prepare()?;
            let registry = KeyRegistry::load(&assets.ansible_dir().join("keys.yml"))?;
            select_registry_key(&registry, "Select key to set")?
        }
    };
    let value = match value {
        Some(v) => v,
        None => {
            let current = config.get(&key).unwrap_or_default();
            Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Value for '{}'", key))
                .default(current)
                .allow_empty(true)
                .interact_text()?
        }
    };
    config.set(&key, &value)?;
    output::success(&format!("{} = {}", key, value));
    Ok(())
}

pub fn run_config_get(key: Option<String>, resolved: bool) -> Result<()> {
    let config = Config::load()?;
    let key = resolve_key(key, &config, "Select key to get")?;
    println!("{}", get_value(&config, &key, resolved)?);
    Ok(())
}

fn get_value(config: &Config, key: &str, resolved: bool) -> Result<String> {
    let value = if resolved {
        config.get_resolved(key)?
    } else {
        config.get(key)
    };
    value.ok_or_else(|| eyre::eyre!("Key '{}' not found", key))
}

pub fn run_config_list() -> Result<()> {
    let config = Config::load()?;
    for (key, value) in config.keys_redacted() {
        println!("{} = {}", key, value);
    }
    Ok(())
}

pub fn run_config_remove(key: Option<String>) -> Result<()> {
    let mut config = Config::load()?;
    let key = resolve_key(key, &config, "Select key to remove")?;
    if config.remove(&key)? {
        output::success(&format!("Removed '{}'", key));
    } else {
        eyre::bail!("Key '{}' not found", key);
    }
    Ok(())
}

pub fn run_config_edit() -> Result<()> {
    let path = Config::path()?;
    if !path.exists() {
        eyre::bail!(
            "Config not found at {}. Run `auberge config init --output {}` first.",
            path.display(),
            path.display()
        );
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| eyre::eyre!("Failed to open editor '{}': {}", editor, e))?;
    Ok(())
}

pub fn run_config_path() -> Result<()> {
    println!("{}", Config::path()?.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_REGISTRY: &str = r#"
keys:
  admin_user_name:
    secret: false
    doc: "Admin username"
  domain:
    secret: false
    doc: "Primary domain"
  tailscale_authkey:
    secret: true
    doc: "Tailscale auth key"
  paperless_admin_password:
    secret: true
    doc: "Paperless admin password"
"#;

    fn fixture_registry() -> (tempfile::TempDir, KeyRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.yml");
        fs::write(&path, FIXTURE_REGISTRY).unwrap();
        let registry = KeyRegistry::load(&path).unwrap();
        (dir, registry)
    }

    /// An ansible dir holding the fixture Key Registry and the given Metas, so
    /// `build_scaffold` resolves them through the same seam a deploy uses.
    fn fixture_ansible_dir(metas: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("keys.yml"), FIXTURE_REGISTRY).unwrap();
        let playbooks = dir.path().join("playbooks");
        fs::create_dir_all(&playbooks).unwrap();
        for (name, body) in metas {
            fs::write(playbooks.join(format!("{name}.meta.yml")), body).unwrap();
        }
        dir
    }

    #[test]
    fn test_build_scaffold_without_playbooks_includes_all_keys() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_ansible_dir(&[]);
        let scaffold = build_scaffold(&registry, &[], dir.path()).unwrap();
        assert!(scaffold.contains("admin_user_name"));
        assert!(scaffold.contains("domain"));
        assert!(scaffold.contains("tailscale_authkey"));
        assert!(scaffold.contains("paperless_admin_password"));
    }

    #[test]
    fn test_build_scaffold_with_playbooks_emits_union_of_required_keys() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_ansible_dir(&[
            (
                "infra",
                "required_keys: [admin_user_name, tailscale_authkey]\n",
            ),
            ("apps", "required_keys: [admin_user_name, domain]\n"),
        ]);
        let scaffold = build_scaffold(
            &registry,
            &["infra".to_string(), "apps".to_string()],
            dir.path(),
        )
        .unwrap();
        assert!(scaffold.contains("admin_user_name"));
        assert!(scaffold.contains("domain"));
        assert!(scaffold.contains("tailscale_authkey"));
        assert!(!scaffold.contains("paperless_admin_password"));
    }

    #[test]
    fn test_build_scaffold_with_unknown_playbook_errors() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_ansible_dir(&[]);
        let err = build_scaffold(&registry, &["nope".to_string()], dir.path()).unwrap_err();
        assert!(err.to_string().contains("Unknown playbook 'nope'"));
    }

    #[test]
    fn test_build_scaffold_with_playbook_having_empty_required_keys_emits_empty() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_ansible_dir(&[("solo", "required_keys: []\n")]);
        let scaffold = build_scaffold(&registry, &["solo".to_string()], dir.path()).unwrap();
        assert!(scaffold.is_empty());
    }

    #[test]
    fn test_write_scaffold_creates_file_with_0600_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        write_scaffold(&path, "domain = \"\"\n", false).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_write_scaffold_refuses_to_overwrite_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "existing content").unwrap();
        let err = write_scaffold(&path, "new content", false).unwrap_err();
        assert!(err.to_string().contains("Refusing to overwrite"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "existing content");
    }

    #[test]
    fn test_write_scaffold_overwrites_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "existing").unwrap();
        write_scaffold(&path, "fresh", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh");
    }

    #[test]
    fn test_sorted_registry_keys_returns_all_keys_alphabetically() {
        let (_keys_dir, registry) = fixture_registry();
        let keys = sorted_registry_keys(&registry);
        assert_eq!(
            keys,
            vec![
                "admin_user_name",
                "domain",
                "paperless_admin_password",
                "tailscale_authkey",
            ]
        );
    }

    fn ref_config() -> Config {
        Config::from_toml_str(
            r#"
            domain = "example.com"
            restic_password = "!echo resolved_secret"
            padded = "!printf '  trimmed  '"
            escaped = "!!pa foo"
            broken = "!false"
            silent = "!true"
        "#,
        )
        .unwrap()
    }

    #[test]
    fn test_get_value_raw_literal() {
        assert_eq!(
            get_value(&ref_config(), "domain", false).unwrap(),
            "example.com"
        );
    }

    #[test]
    fn test_get_value_raw_ref_stays_raw() {
        assert_eq!(
            get_value(&ref_config(), "restic_password", false).unwrap(),
            "!echo resolved_secret"
        );
    }

    #[test]
    fn test_get_value_resolved_literal_is_identity() {
        assert_eq!(
            get_value(&ref_config(), "domain", true).unwrap(),
            "example.com"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_value_resolved_ref_runs_command() {
        assert_eq!(
            get_value(&ref_config(), "restic_password", true).unwrap(),
            "resolved_secret"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_value_resolved_trims_whitespace() {
        assert_eq!(get_value(&ref_config(), "padded", true).unwrap(), "trimmed");
    }

    #[cfg(unix)]
    #[test]
    fn test_get_value_resolved_escaped_bang_is_literal() {
        assert_eq!(
            get_value(&ref_config(), "escaped", true).unwrap(),
            "!pa foo"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_value_resolved_failing_command_errors() {
        let err = get_value(&ref_config(), "broken", true).unwrap_err();
        let chain = format!("{err:#}");
        assert!(chain.contains("Failed to resolve config key 'broken'"));
        assert!(chain.contains("Shell command failed"));
    }

    #[cfg(unix)]
    #[test]
    fn test_get_value_resolved_empty_output_errors() {
        let err = get_value(&ref_config(), "silent", true).unwrap_err();
        let chain = format!("{err:#}");
        assert!(chain.contains("Failed to resolve config key 'silent'"));
        assert!(chain.contains("empty output"));
    }

    #[test]
    fn test_get_value_missing_key_errors_in_both_modes() {
        for resolved in [false, true] {
            let err = get_value(&ref_config(), "nope", resolved).unwrap_err();
            assert!(err.to_string().contains("Key 'nope' not found"));
        }
    }

    #[test]
    fn test_write_scaffold_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sub/config.toml");
        write_scaffold(&path, "domain = \"\"\n", false).unwrap();
        assert!(path.exists());
    }
}
