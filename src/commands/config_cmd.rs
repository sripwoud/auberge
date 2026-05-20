use crate::ansible_assets::AnsibleAssets;
use crate::config::Config;
use crate::key_registry::KeyRegistry;
use crate::output;
use crate::playbook_meta::PlaybookMeta;
use crate::prompt::select_item;
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
        alias = "i",
        about = "Print a config.toml scaffold derived from the Key Registry"
    )]
    Init(InitArgs),
    #[command(alias = "s", about = "Set a config value")]
    Set {
        #[arg(help = "Key name (e.g. admin_user_name)")]
        key: Option<String>,
        #[arg(help = "Value to set")]
        value: Option<String>,
    },
    #[command(alias = "g", about = "Get a config value")]
    Get {
        #[arg(help = "Key name")]
        key: Option<String>,
    },
    #[command(
        alias = "l",
        about = "List all config keys (sensitive values redacted)"
    )]
    List,
    #[command(alias = "rm", about = "Remove a key from config")]
    Remove {
        #[arg(help = "Key name")]
        key: Option<String>,
    },
    #[command(alias = "e", about = "Open config in $EDITOR")]
    Edit,
    #[command(alias = "p", about = "Print config file path")]
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

fn select_key(config: &Config, prompt: &str) -> Result<String> {
    let keys = config.keys();
    if keys.is_empty() {
        eyre::bail!("No config keys found");
    }
    select_item(&keys, |s: &String| s.clone(), prompt)?
        .ok_or_else(|| eyre::eyre!("No key selected"))
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
    select_item(&keys, display, prompt)?.ok_or_else(|| eyre::eyre!("No key selected"))
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
    let scaffold = build_scaffold(&registry, &args.playbooks, &assets.playbooks_dir())?;

    match args.output {
        None => {
            print!("{scaffold}");
            Ok(())
        }
        Some(path) => write_scaffold(&path, &scaffold, args.force),
    }
}

fn build_scaffold(
    registry: &KeyRegistry,
    playbooks: &[String],
    playbooks_dir: &Path,
) -> Result<String> {
    if playbooks.is_empty() {
        return Ok(registry.scaffold());
    }
    let mut keys: HashSet<String> = HashSet::new();
    for playbook in playbooks {
        let meta_path = playbooks_dir.join(format!("{playbook}.meta.yml"));
        let meta = PlaybookMeta::load(&meta_path)
            .wrap_err_with(|| format!("Unknown playbook '{playbook}'"))?;
        keys.extend(meta.required_keys);
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

pub fn run_config_get(key: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let key = resolve_key(key, &config, "Select key to get")?;
    match config.get(&key) {
        Some(value) => println!("{}", value),
        None => eyre::bail!("Key '{}' not found", key),
    }
    Ok(())
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

    fn fixture_registry() -> (tempfile::TempDir, KeyRegistry) {
        let yaml = r#"
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.yml");
        fs::write(&path, yaml).unwrap();
        let registry = KeyRegistry::load(&path).unwrap();
        (dir, registry)
    }

    fn fixture_playbooks_dir(metas: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in metas {
            fs::write(dir.path().join(format!("{name}.meta.yml")), body).unwrap();
        }
        dir
    }

    #[test]
    fn test_build_scaffold_without_playbooks_includes_all_keys() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_playbooks_dir(&[]);
        let scaffold = build_scaffold(&registry, &[], dir.path()).unwrap();
        assert!(scaffold.contains("admin_user_name"));
        assert!(scaffold.contains("domain"));
        assert!(scaffold.contains("tailscale_authkey"));
        assert!(scaffold.contains("paperless_admin_password"));
    }

    #[test]
    fn test_build_scaffold_with_playbooks_emits_union_of_required_keys() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_playbooks_dir(&[
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
        let dir = fixture_playbooks_dir(&[]);
        let err = build_scaffold(&registry, &["nope".to_string()], dir.path()).unwrap_err();
        assert!(err.to_string().contains("Unknown playbook 'nope'"));
    }

    #[test]
    fn test_build_scaffold_with_playbook_having_empty_required_keys_emits_empty() {
        let (_keys_dir, registry) = fixture_registry();
        let dir = fixture_playbooks_dir(&[("solo", "required_keys: []\n")]);
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

    #[test]
    fn test_write_scaffold_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sub/config.toml");
        write_scaffold(&path, "domain = \"\"\n", false).unwrap();
        assert!(path.exists());
    }
}
