use crate::models::inventory::Host;
use crate::selector::select_item;
use crate::services::inventory::get_hosts;
use chrono::Utc;
use clap::Subcommand;
use eyre::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let backup_root = default_backup_dir();

    if !backup_root.exists() {
        eprintln!("No backups found. Backup directory does not exist:");
        eprintln!("  {}", backup_root.display());
        return Ok(());
    }

    let backups = discover_backups(&backup_root, host_filter.as_deref(), app_filter.as_deref())?;

    if backups.is_empty() {
        eprintln!("No backups found");
        return Ok(());
    }

    match format {
        OutputFormat::Table => print_backups_table(&backups),
        OutputFormat::Json => print_backups_json(&backups)?,
        OutputFormat::Yaml => print_backups_yaml(&backups)?,
    }

    Ok(())
}

#[derive(Debug)]
struct BackupEntry {
    host: String,
    app: String,
    timestamp: String,
    path: PathBuf,
    size_bytes: u64,
}

fn discover_backups(
    backup_root: &Path,
    host_filter: Option<&str>,
    app_filter: Option<&str>,
) -> Result<Vec<BackupEntry>> {
    let mut backups = Vec::new();

    if !backup_root.is_dir() {
        return Ok(backups);
    }

    for host_entry in fs::read_dir(backup_root)
        .wrap_err_with(|| format!("Failed to read backup directory: {}", backup_root.display()))?
    {
        let host_entry = host_entry?;
        if !host_entry.file_type()?.is_dir() {
            continue;
        }

        let host_name = host_entry.file_name().to_string_lossy().to_string();

        if let Some(filter) = host_filter
            && host_name != filter
        {
            continue;
        }

        for app_entry in fs::read_dir(host_entry.path())? {
            let app_entry = app_entry?;
            if !app_entry.file_type()?.is_dir() {
                continue;
            }

            let app_name = app_entry.file_name().to_string_lossy().to_string();

            if let Some(filter) = app_filter
                && app_name != filter
            {
                continue;
            }

            for backup_entry in fs::read_dir(app_entry.path())? {
                let backup_entry = backup_entry?;
                let backup_path = backup_entry.path();

                if backup_path.is_symlink() {
                    continue;
                }

                if !backup_path.is_dir() {
                    continue;
                }

                let timestamp = backup_entry.file_name().to_string_lossy().to_string();
                let size_bytes = calculate_dir_size(&backup_path)?;

                backups.push(BackupEntry {
                    host: host_name.clone(),
                    app: app_name.clone(),
                    timestamp,
                    path: backup_path,
                    size_bytes,
                });
            }
        }
    }

    backups.sort_by(|a, b| {
        a.host
            .cmp(&b.host)
            .then_with(|| a.app.cmp(&b.app))
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    Ok(backups)
}

fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;

    if path.is_file() {
        return Ok(path.metadata()?.len());
    }

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;

            if metadata.is_file() {
                total += metadata.len();
            } else if metadata.is_dir() {
                total += calculate_dir_size(&entry.path())?;
            }
        }
    }

    Ok(total)
}

fn print_backups_table(backups: &[BackupEntry]) {
    println!(
        "{:<15} {:<12} {:<20} {:<12}",
        "HOST", "APP", "TIMESTAMP", "SIZE"
    );
    println!("{}", "-".repeat(65));

    for backup in backups {
        println!(
            "{:<15} {:<12} {:<20} {:<12}",
            backup.host,
            backup.app,
            backup.timestamp,
            format_size(backup.size_bytes)
        );
    }

    println!("\nTotal: {} backup(s)", backups.len());
}

fn print_backups_json(backups: &[BackupEntry]) -> Result<()> {
    let json = serde_json::to_string_pretty(
        &backups
            .iter()
            .map(|b| {
                serde_json::json!({
                    "host": b.host,
                    "app": b.app,
                    "timestamp": b.timestamp,
                    "path": b.path,
                    "size_bytes": b.size_bytes,
                })
            })
            .collect::<Vec<_>>(),
    )?;

    println!("{}", json);
    Ok(())
}

fn print_backups_yaml(backups: &[BackupEntry]) -> Result<()> {
    let yaml = serde_yaml::to_string(
        &backups
            .iter()
            .map(|b| {
                serde_yaml::to_value(serde_json::json!({
                    "host": b.host,
                    "app": b.app,
                    "timestamp": b.timestamp,
                    "path": b.path,
                    "size_bytes": b.size_bytes,
                }))
                .unwrap()
            })
            .collect::<Vec<_>>(),
    )?;

    println!("{}", yaml);
    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn run_backup_restore(
    backup_id: String,
    host_arg: Option<String>,
    apps: Option<Vec<String>>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let host = get_host_or_select(host_arg)?;
    let backup_root = default_backup_dir();
    let host_backup_dir = backup_root.join(&host.name);

    if !host_backup_dir.exists() {
        eyre::bail!("No backups found for host: {}", host.name);
    }

    let app_names = apps.unwrap_or_else(|| {
        vec![
            "radicale".to_string(),
            "freshrss".to_string(),
            "navidrome".to_string(),
            "calibre".to_string(),
            "webdav".to_string(),
        ]
    });

    let mut restore_plan = Vec::new();

    for app_name in &app_names {
        let app_backup_dir = host_backup_dir.join(app_name);

        if !app_backup_dir.exists() {
            eprintln!("⚠ No backups found for {}, skipping", app_name);
            continue;
        }

        let backup_path = if backup_id == "latest" {
            let latest_link = app_backup_dir.join("latest");
            if !latest_link.exists() {
                eprintln!("⚠ No 'latest' backup for {}, skipping", app_name);
                continue;
            }
            fs::canonicalize(latest_link)?
        } else {
            let backup_path = app_backup_dir.join(&backup_id);
            if !backup_path.exists() {
                eprintln!(
                    "⚠ Backup {} not found for {}, skipping",
                    backup_id, app_name
                );
                continue;
            }
            backup_path
        };

        restore_plan.push((app_name.clone(), backup_path));
    }

    if restore_plan.is_empty() {
        eyre::bail!("No backups to restore");
    }

    eprintln!("\n=== Restore Plan ===");
    eprintln!("Host: {}", host.name);
    eprintln!("Backup ID: {}", backup_id);
    eprintln!("\nApps to restore:");
    for (app, path) in &restore_plan {
        eprintln!("  - {:<12} from {}", app, path.display());
    }

    if dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(());
    }

    if !yes {
        eprintln!("\n⚠ WARNING: This will overwrite existing data on the remote host!");
        if !dialoguer::Confirm::new()
            .with_prompt("Continue with restore?")
            .default(false)
            .interact()?
        {
            eprintln!("Restore cancelled");
            return Ok(());
        }
    }

    eprintln!("\nStarting restore...");

    for (app_name, backup_path) in restore_plan {
        restore_app(&host, &app_name, &backup_path)?;
    }

    eprintln!("\n✓ All restores completed successfully");
    Ok(())
}

fn restore_app(host: &Host, app_name: &str, backup_path: &Path) -> Result<()> {
    eprintln!("\n--- Restoring {} ---", app_name);

    let config = AppBackupConfig::by_name(app_name, false)
        .ok_or_else(|| eyre::eyre!("Unknown app: {}", app_name))?;

    let ssh_key = get_ssh_key_path(host)?;

    if let Some(service) = config.systemd_service {
        eprintln!("  Stopping service: {}", service);
        remote_systemctl(host, &ssh_key, "stop", service)?;
    }

    for remote_path in &config.paths {
        eprintln!("  Restoring to: {}", remote_path);
        rsync_to_remote(host, &ssh_key, backup_path, remote_path)?;
    }

    if let Some(service) = config.systemd_service {
        eprintln!("  Starting service: {}", service);
        remote_systemctl(host, &ssh_key, "start", service)?;
    }

    eprintln!("✓ {} restore completed", app_name);
    Ok(())
}

fn rsync_to_remote(
    host: &Host,
    ssh_key: &Path,
    local_path: &Path,
    remote_path: &str,
) -> Result<()> {
    let local_source = local_path.join(remote_path.trim_start_matches('/'));

    if !local_source.exists() {
        eprintln!("    (skipping {} - not in backup)", remote_path);
        return Ok(());
    }

    let parent_dir = std::path::Path::new(remote_path)
        .parent()
        .ok_or_else(|| eyre::eyre!("Invalid remote path: {}", remote_path))?;

    let _ = Command::new("ssh")
        .arg("-i")
        .arg(ssh_key)
        .arg("-p")
        .arg(host.vars.ansible_port.to_string())
        .arg(format!("ansible@{}", host.vars.ansible_host))
        .arg("sudo")
        .arg("mkdir")
        .arg("-p")
        .arg(parent_dir)
        .status();

    let status = Command::new("rsync")
        .arg("-avz")
        .arg("--delete")
        .arg("-e")
        .arg(format!(
            "ssh -i {} -p {}",
            ssh_key.display(),
            host.vars.ansible_port
        ))
        .arg(format!("{}/", local_source.display()))
        .arg(format!(
            "ansible@{}:{}",
            host.vars.ansible_host, remote_path
        ))
        .status()
        .wrap_err("Failed to execute rsync")?;

    if !status.success() {
        eyre::bail!("rsync failed for {}", remote_path);
    }

    Ok(())
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

fn backup_app(host: &Host, config: &AppBackupConfig, backup_dest: &Path) -> Result<()> {
    eprintln!("\n--- Backing up {} ---", config.name);

    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let app_backup_dir = backup_dest
        .join(&host.name)
        .join(config.name)
        .join(&timestamp);

    fs::create_dir_all(&app_backup_dir).wrap_err_with(|| {
        format!(
            "Failed to create backup directory: {}",
            app_backup_dir.display()
        )
    })?;

    let ssh_key = get_ssh_key_path(host)?;

    if let Some(service) = config.systemd_service {
        eprintln!("  Stopping service: {}", service);
        remote_systemctl(host, &ssh_key, "stop", service)?;
    }

    for path in &config.paths {
        eprintln!("  Backing up: {}", path);
        rsync_from_remote(host, &ssh_key, path, &app_backup_dir)?;
    }

    if let Some(service) = config.systemd_service {
        eprintln!("  Starting service: {}", service);
        remote_systemctl(host, &ssh_key, "start", service)?;
    }

    let latest_link = backup_dest
        .join(&host.name)
        .join(config.name)
        .join("latest");
    if latest_link.exists() || latest_link.is_symlink() {
        let _ = fs::remove_file(&latest_link);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&timestamp, &latest_link).wrap_err("Failed to create 'latest' symlink")?;
    }

    eprintln!("✓ {} backup completed", config.name);
    eprintln!("  Location: {}", app_backup_dir.display());
    Ok(())
}

fn get_ssh_key_path(host: &Host) -> Result<PathBuf> {
    let ansible_user = "ansible";
    let ssh_key = dirs::home_dir()
        .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
        .join(format!(".ssh/identities/{}_{}", ansible_user, host.name));

    if !ssh_key.exists() {
        eyre::bail!(
            "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}' first",
            ssh_key.display(),
            host.name,
            ansible_user
        );
    }

    Ok(ssh_key)
}

fn remote_systemctl(host: &Host, ssh_key: &Path, action: &str, service: &str) -> Result<()> {
    let status = Command::new("ssh")
        .arg("-i")
        .arg(ssh_key)
        .arg("-p")
        .arg(host.vars.ansible_port.to_string())
        .arg(format!("ansible@{}", host.vars.ansible_host))
        .arg("sudo")
        .arg("systemctl")
        .arg(action)
        .arg(service)
        .status()
        .wrap_err_with(|| format!("Failed to {} service {}", action, service))?;

    if !status.success() {
        eyre::bail!("systemctl {} {} failed", action, service);
    }

    Ok(())
}

fn rsync_from_remote(
    host: &Host,
    ssh_key: &Path,
    remote_path: &str,
    local_dest: &Path,
) -> Result<()> {
    let status = Command::new("rsync")
        .arg("-avz")
        .arg("--relative")
        .arg("-e")
        .arg(format!(
            "ssh -i {} -p {}",
            ssh_key.display(),
            host.vars.ansible_port
        ))
        .arg(format!(
            "ansible@{}:{}",
            host.vars.ansible_host, remote_path
        ))
        .arg(local_dest)
        .status()
        .wrap_err("Failed to execute rsync")?;

    if !status.success() {
        eyre::bail!("rsync failed for {}", remote_path);
    }

    Ok(())
}
