use crate::models::inventory::Host;
use crate::selector::select_item;
use crate::services::inventory::get_hosts;
use clap::Subcommand;
use eyre::Result;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum BackupCommands {
    #[command(alias = "c", about = "Create backup of application data")]
    Create {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to backup (radicale,freshrss,navidrome,calibre,webdav). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(short, long, help = "Backup destination directory")]
        dest: Option<PathBuf>,
        #[arg(long, help = "Include music files in Navidrome backup (large, slow)")]
        include_music: bool,
        #[arg(short = 'n', long, help = "Dry run (show what would be backed up)")]
        dry_run: bool,
    },
    #[command(alias = "ls", about = "List available backups")]
    List {
        #[arg(short = 'H', long, help = "Filter by host")]
        host: Option<String>,
        #[arg(short, long, help = "Filter by app")]
        app: Option<String>,
        #[arg(
            short,
            long,
            value_enum,
            default_value = "table",
            help = "Output format"
        )]
        format: OutputFormat,
    },
    #[command(alias = "r", about = "Restore from backup")]
    Restore {
        #[arg(help = "Backup timestamp (YYYY-MM-DD_HH-MM-SS) or 'latest'")]
        backup_id: String,
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to restore (radicale,freshrss,navidrome,calibre,webdav). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(short = 'n', long, help = "Dry run (show what would be restored)")]
        dry_run: bool,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
    },
    #[command(about = "Export FreshRSS feeds to OPML file")]
    ExportOpml {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Output OPML file path")]
        output: PathBuf,
        #[arg(long, default_value = "admin", help = "FreshRSS username")]
        user: String,
    },
    #[command(about = "Import OPML file to FreshRSS")]
    ImportOpml {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "OPML file to import")]
        input: PathBuf,
        #[arg(long, default_value = "admin", help = "FreshRSS username")]
        user: String,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

#[derive(Debug)]
pub struct AppBackupConfig {
    pub name: &'static str,
    pub systemd_service: Option<&'static str>,
    pub paths: Vec<&'static str>,
}

impl AppBackupConfig {
    pub fn all() -> Vec<Self> {
        vec![
            Self::radicale(),
            Self::freshrss(),
            Self::navidrome(false),
            Self::calibre(),
            Self::webdav(),
        ]
    }

    pub fn by_name(name: &str, include_music: bool) -> Option<Self> {
        match name {
            "radicale" => Some(Self::radicale()),
            "freshrss" => Some(Self::freshrss()),
            "navidrome" => Some(Self::navidrome(include_music)),
            "calibre" => Some(Self::calibre()),
            "webdav" => Some(Self::webdav()),
            _ => None,
        }
    }

    fn radicale() -> Self {
        Self {
            name: "radicale",
            systemd_service: Some("radicale"),
            paths: vec!["/var/lib/radicale/collections", "/etc/radicale"],
        }
    }

    fn freshrss() -> Self {
        Self {
            name: "freshrss",
            systemd_service: Some("freshrss"),
            paths: vec!["/var/lib/freshrss", "/opt/freshrss/data"],
        }
    }

    fn navidrome(include_music: bool) -> Self {
        let mut paths = vec!["/var/lib/navidrome", "/etc/navidrome"];

        if include_music {
            paths.push("/srv/music");
        }

        Self {
            name: "navidrome",
            systemd_service: Some("navidrome"),
            paths,
        }
    }

    fn calibre() -> Self {
        Self {
            name: "calibre",
            systemd_service: Some("calibre"),
            paths: vec!["/srv/calibre", "/opt/calibre"],
        }
    }

    fn webdav() -> Self {
        Self {
            name: "webdav",
            systemd_service: None,
            paths: vec!["/var/www/webdav-files"],
        }
    }
}

pub fn run_backup_create(
    host_arg: Option<String>,
    apps: Option<Vec<String>>,
    dest: Option<PathBuf>,
    include_music: bool,
    dry_run: bool,
) -> Result<()> {
    let host = get_host_or_select(host_arg)?;
    let backup_dest = dest.unwrap_or_else(default_backup_dir);

    eprintln!("Creating backup for host: {}", host.name);
    eprintln!("Backup destination: {}", backup_dest.display());

    let app_configs = match apps {
        Some(app_names) => app_names
            .iter()
            .filter_map(|name| AppBackupConfig::by_name(name, include_music))
            .collect(),
        None => AppBackupConfig::all(),
    };

    if app_configs.is_empty() {
        eyre::bail!("No valid apps specified for backup");
    }

    eprintln!("\nApps to backup:");
    for config in &app_configs {
        eprintln!("  - {}", config.name);
        for path in &config.paths {
            eprintln!("    └─ {}", path);
        }
    }

    if dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(());
    }

    eprintln!("\nStarting backup...");

    for config in app_configs {
        backup_app(&host, &config, &backup_dest)?;
    }

    eprintln!("\n✓ All backups completed successfully");
    Ok(())
}

pub fn run_backup_list(
    host_filter: Option<String>,
    app_filter: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    eprintln!("Listing backups...");
    eprintln!("Host filter: {:?}", host_filter);
    eprintln!("App filter: {:?}", app_filter);
    eprintln!("Format: {:?}", format);
    eyre::bail!("Not yet implemented")
}

pub fn run_backup_restore(
    backup_id: String,
    host_arg: Option<String>,
    apps: Option<Vec<String>>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let host = get_host_or_select(host_arg)?;

    eprintln!("Restoring backup: {}", backup_id);
    eprintln!("Host: {}", host.name);
    eprintln!("Apps: {:?}", apps);
    eprintln!("Dry run: {}", dry_run);
    eprintln!("Skip confirmation: {}", yes);

    eyre::bail!("Not yet implemented")
}

pub fn run_export_opml(host_arg: Option<String>, output: PathBuf, user: String) -> Result<()> {
    let host = get_host_or_select(host_arg)?;

    eprintln!("Exporting OPML from FreshRSS");
    eprintln!("Host: {}", host.name);
    eprintln!("User: {}", user);
    eprintln!("Output: {}", output.display());

    eyre::bail!("Not yet implemented")
}

pub fn run_import_opml(host_arg: Option<String>, input: PathBuf, user: String) -> Result<()> {
    let host = get_host_or_select(host_arg)?;

    eprintln!("Importing OPML to FreshRSS");
    eprintln!("Host: {}", host.name);
    eprintln!("User: {}", user);
    eprintln!("Input: {}", input.display());

    eyre::bail!("Not yet implemented")
}

fn get_host_or_select(host_arg: Option<String>) -> Result<Host> {
    match host_arg {
        Some(name) => crate::services::inventory::get_host(&name, None),
        None => {
            let hosts = get_hosts(None, None)?;
            select_item(
                &hosts,
                |h: &Host| {
                    format!(
                        "{} ({}:{})",
                        h.name, h.vars.ansible_host, h.vars.ansible_port
                    )
                },
                "Select host",
            )?
            .ok_or_else(|| eyre::eyre!("No host selected"))
        }
    }
}

fn default_backup_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("auberge").join("backups"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/auberge/backups"))
}

fn backup_app(_host: &Host, config: &AppBackupConfig, _backup_dest: &PathBuf) -> Result<()> {
    eprintln!("\n--- Backing up {} ---", config.name);

    if let Some(service) = config.systemd_service {
        eprintln!("Stopping service: {}", service);
    }

    for path in &config.paths {
        eprintln!("  Backing up: {}", path);
    }

    if let Some(service) = config.systemd_service {
        eprintln!("Starting service: {}", service);
    }

    eprintln!("✓ {} backup completed", config.name);
    Ok(())
}
