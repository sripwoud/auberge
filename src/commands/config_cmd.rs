use crate::output;
use crate::user_config::UserConfig;
use clap::Subcommand;
use eyre::Result;

#[derive(Subcommand)]
pub enum ConfigCommands {
    #[command(about = "Create template config.toml with all known keys")]
    Init,
    #[command(about = "Set a config value")]
    Set {
        #[arg(help = "Key name (e.g. admin_user_name)")]
        key: String,
        #[arg(help = "Value to set")]
        value: String,
    },
    #[command(about = "Get a config value")]
    Get {
        #[arg(help = "Key name")]
        key: String,
    },
    #[command(about = "List all config keys (sensitive values redacted)")]
    List,
    #[command(about = "Remove a key from config")]
    Remove {
        #[arg(help = "Key name")]
        key: String,
    },
    #[command(about = "Open config in $EDITOR")]
    Edit,
    #[command(about = "Print config file path")]
    Path,
}

pub fn run_config_init() -> Result<()> {
    let path = UserConfig::init()?;
    output::success(&format!("Created config at {}", path.display()));
    Ok(())
}

pub fn run_config_set(key: String, value: String) -> Result<()> {
    let mut config = UserConfig::load()?;
    if config.set(&key, &value)? {
        output::success(&format!("{} = {}", key, value));
    } else {
        eyre::bail!("Key '{}' not found in any config section", key);
    }
    Ok(())
}

pub fn run_config_get(key: String) -> Result<()> {
    let config = UserConfig::load()?;
    match config.get(&key) {
        Some(value) => println!("{}", value),
        None => eyre::bail!("Key '{}' not found", key),
    }
    Ok(())
}

pub fn run_config_list() -> Result<()> {
    let config = UserConfig::load()?;
    for (key, value) in config.keys_redacted() {
        println!("{} = {}", key, value);
    }
    Ok(())
}

pub fn run_config_remove(key: String) -> Result<()> {
    let mut config = UserConfig::load()?;
    if config.remove(&key)? {
        output::success(&format!("Removed '{}'", key));
    } else {
        eyre::bail!("Key '{}' not found", key);
    }
    Ok(())
}

pub fn run_config_edit() -> Result<()> {
    let path = UserConfig::path()?;
    if !path.exists() {
        eyre::bail!(
            "Config not found at {}. Run `auberge config init` first.",
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
    println!("{}", UserConfig::path()?.display());
    Ok(())
}
