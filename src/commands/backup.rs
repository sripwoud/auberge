use crate::hosts::{Host, HostManager};
use crate::output;
use crate::selector::select_item;
use crate::user_config::UserConfig;
use chrono::Utc;
use clap::Subcommand;
use eyre::{Context, Result};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Instant;
use tabled::Tabled;

struct SshSession<'a> {
    host: &'a Host,
    ssh_key: &'a Path,
}

impl<'a> SshSession<'a> {
    fn new(host: &'a Host, ssh_key: &'a Path) -> Self {
        Self { host, ssh_key }
    }

    fn ssh_args(&self) -> Vec<OsString> {
        vec![
            "-o".into(),
            "ControlMaster=auto".into(),
            "-o".into(),
            "ControlPath=/tmp/ssh-%r@%h:%p".into(),
            "-o".into(),
            "ControlPersist=60s".into(),
            "-i".into(),
            self.ssh_key.into(),
            "-p".into(),
            self.host.port.to_string().into(),
            format!("{}@{}", self.host.user, self.host.address).into(),
        ]
    }

    fn run(&self, command: &str) -> Result<Output> {
        Command::new("ssh")
            .args(self.ssh_args())
            .arg(command)
            .output()
            .wrap_err("Failed to execute SSH command")
    }

    fn run_raw(&self, args: &[&str]) -> Result<Output> {
        let mut cmd = Command::new("ssh");
        cmd.args(self.ssh_args());
        for arg in args {
            cmd.arg(arg);
        }
        cmd.output().wrap_err("Failed to execute SSH command")
    }

    fn rsync_e_arg(&self) -> String {
        format!(
            "ssh -o ControlMaster=auto -o ControlPath=/tmp/ssh-%r@%h:%p -o ControlPersist=60s -i {} -p {}",
            self.ssh_key.display(),
            self.host.port
        )
    }

    fn scp_args(&self) -> Vec<OsString> {
        vec![
            "-o".into(),
            "ControlMaster=auto".into(),
            "-o".into(),
            "ControlPath=/tmp/ssh-%r@%h:%p".into(),
            "-o".into(),
            "ControlPersist=60s".into(),
            "-i".into(),
            self.ssh_key.into(),
            "-P".into(),
            self.host.port.to_string().into(),
        ]
    }

    fn scp_to(&self, local: &Path, remote: &str) -> Result<()> {
        let status = Command::new("scp")
            .args(self.scp_args())
            .arg(local)
            .arg(format!(
                "{}@{}:{}",
                self.host.user, self.host.address, remote
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .wrap_err("Failed to upload file via scp")?;
        if !status.success() {
            eyre::bail!("scp to {}:{} failed", self.host.address, remote);
        }
        Ok(())
    }

    fn scp_from(&self, remote: &str, local: &Path) -> Result<()> {
        let status = Command::new("scp")
            .args(self.scp_args())
            .arg(format!(
                "{}@{}:{}",
                self.host.user, self.host.address, remote
            ))
            .arg(local)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .wrap_err("Failed to download file via scp")?;
        if !status.success() {
            eyre::bail!("scp from {}:{} failed", self.host.address, remote);
        }
        Ok(())
    }

    fn systemctl(&self, action: &str, service: &str) -> Result<()> {
        let status = Command::new("ssh")
            .args(self.ssh_args())
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
}

const RSYNC_EXCLUDES: &[&str] = &[
    ".git",
    ".git/",
    "venv",
    "venv/",
    "node_modules",
    "node_modules/",
    "__pycache__",
    "__pycache__/",
    "*.pyc",
    "*.pyo",
    ".cache",
    ".cache/",
    ".Radicale.cache",
    ".Radicale.cache/",
    "*.tmp",
    "*.log",
    ".DS_Store",
];

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
            help = "Apps to backup (baikal,freshrss,navidrome,calibre,webdav,yourls,paperless). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(short, long, help = "Backup destination directory")]
        dest: Option<PathBuf>,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{user}_{host})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(long, help = "Include music files in Navidrome backup (large, slow)")]
        include_music: bool,
        #[arg(short = 'n', long, help = "Dry run (show what would be backed up)")]
        dry_run: bool,
        #[arg(short, long, help = "Show detailed progress and paths")]
        verbose: bool,
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
            short = 'F',
            long,
            help = "Source host (for cross-host restore/migration)"
        )]
        from_host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to restore (baikal,freshrss,navidrome,calibre,webdav,yourls,paperless). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{user}_{host})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(short = 'n', long, help = "Dry run (show what would be restored)")]
        dry_run: bool,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
        #[arg(
            long,
            help = "UNSAFE: Skip Ansible playbook run (services will fail without correct permissions)"
        )]
        skip_playbook_unsafe: bool,
    },
    #[command(alias = "eo", about = "Export FreshRSS feeds to OPML file")]
    ExportOpml {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Output OPML file path")]
        output: PathBuf,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{user}_{host})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(long, default_value = "admin", help = "FreshRSS username")]
        user: String,
    },
    #[command(alias = "p", about = "Push backups to offsite restic repository")]
    Push {
        #[arg(short = 'H', long, help = "Filter backups by host")]
        host: Option<String>,
        #[arg(short, long, help = "Specific backup timestamp (default: latest)")]
        backup_id: Option<String>,
    },
    #[command(about = "Prune old snapshots from offsite restic repository")]
    Prune {
        #[arg(short = 'n', long, help = "Show what would be pruned without removing")]
        dry_run: bool,
    },
    #[command(alias = "io", about = "Import OPML file to FreshRSS")]
    ImportOpml {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "OPML file to import")]
        input: PathBuf,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{user}_{host})"
        )]
        ssh_key: Option<PathBuf>,
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
pub struct DbBackupConfig {
    pub db_name: &'static str,
    pub remote_dump_path: &'static str,
}

pub struct AppBackupConfig {
    pub name: &'static str,
    pub systemd_services: Vec<&'static str>,
    pub paths: Vec<&'static str>,
    pub owner: Option<(&'static str, &'static str)>,
    pub db: Option<DbBackupConfig>,
}

pub struct RestoreOptions {
    pub backup_id: String,
    pub host_arg: Option<String>,
    pub from_host_arg: Option<String>,
    pub apps: Option<Vec<String>>,
    pub ssh_key: Option<PathBuf>,
    pub dry_run: bool,
    pub yes: bool,
    pub skip_playbook_unsafe: bool,
}

impl AppBackupConfig {
    pub fn all() -> Vec<Self> {
        vec![
            Self::baikal(),
            Self::freshrss(),
            Self::navidrome(false),
            Self::calibre(),
            Self::webdav(),
            Self::yourls(),
            Self::paperless(),
        ]
    }

    pub fn by_name(name: &str, include_music: bool) -> Option<Self> {
        match name {
            "baikal" => Some(Self::baikal()),
            "freshrss" => Some(Self::freshrss()),
            "navidrome" => Some(Self::navidrome(include_music)),
            "calibre" => Some(Self::calibre()),
            "webdav" => Some(Self::webdav()),
            "yourls" => Some(Self::yourls()),
            "paperless" => Some(Self::paperless()),
            _ => None,
        }
    }

    fn baikal() -> Self {
        Self {
            name: "baikal",
            systemd_services: vec![],
            paths: vec!["/opt/baikal/Specific"],
            owner: Some(("baikal", "baikal")),
            db: None,
        }
    }

    fn freshrss() -> Self {
        Self {
            name: "freshrss",
            systemd_services: vec!["freshrss"],
            paths: vec!["/var/lib/freshrss", "/opt/freshrss/data"],
            owner: Some(("freshrss", "freshrss")),
            db: None,
        }
    }

    fn navidrome(include_music: bool) -> Self {
        let mut paths = vec!["/var/lib/navidrome", "/etc/navidrome"];

        if include_music {
            paths.push("/srv/music");
        }

        Self {
            name: "navidrome",
            systemd_services: vec!["navidrome"],
            paths,
            owner: Some(("navidrome", "navidrome")),
            db: None,
        }
    }

    fn calibre() -> Self {
        Self {
            name: "calibre",
            systemd_services: vec!["calibre"],
            paths: vec!["/srv/calibre", "/opt/calibre", "/home/calibre"],
            owner: Some(("calibre", "calibre")),
            db: None,
        }
    }

    fn webdav() -> Self {
        Self {
            name: "webdav",
            systemd_services: vec![],
            paths: vec!["/var/www/webdav-files"],
            owner: None,
            db: None,
        }
    }

    fn yourls() -> Self {
        Self {
            name: "yourls",
            systemd_services: vec![],
            paths: vec!["/var/www/yourls"],
            owner: Some(("www-data", "www-data")),
            db: None,
        }
    }

    fn paperless() -> Self {
        Self {
            name: "paperless",
            systemd_services: vec![
                "paperless-webserver",
                "paperless-consumer",
                "paperless-task-queue",
                "paperless-scheduler",
            ],
            paths: vec!["/opt/paperless/data", "/opt/paperless/media"],
            owner: Some(("paperless", "paperless")),
            db: Some(DbBackupConfig {
                db_name: "paperless",
                remote_dump_path: "/tmp/paperless_db.dump",
            }),
        }
    }
}

pub fn run_backup_create(
    host_arg: Option<String>,
    apps: Option<Vec<String>>,
    dest: Option<PathBuf>,
    ssh_key: Option<PathBuf>,
    include_music: bool,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let host = get_host_or_select(host_arg)?;
    let backup_dest = dest.unwrap_or_else(default_backup_dir);

    let ssh_key_path = resolve_ssh_key_path(&host, ssh_key)?;

    if verbose {
        eprintln!("Using SSH key: {}", ssh_key_path.display());
        eprintln!("Backing up to: {}", backup_dest.join(&host.name).display());
    } else {
        let short_dest = backup_dest
            .to_string_lossy()
            .replace(&std::env::var("HOME").unwrap_or_default(), "~");
        eprintln!("Backing up {} → {}", host.name, short_dest);
    }

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

    if verbose {
        let app_names: Vec<&str> = app_configs.iter().map(|c| c.name).collect();
        eprintln!("Apps: {}\n", app_names.join(", "));
    }

    if dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(());
    }
    let start_time = Instant::now();
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    let mut results = Vec::new();
    for config in app_configs {
        match backup_app(
            &host,
            &config,
            &backup_dest,
            &ssh_key_path,
            &timestamp,
            verbose,
        ) {
            Ok(size) => results.push((config.name, true, Some(size), None)),
            Err(e) => {
                eprintln!("✗ {} backup failed: {}", config.name, e);
                results.push((config.name, false, None, Some(e.to_string())));
            }
        }
    }

    let elapsed = start_time.elapsed().as_secs();
    let total_size: u64 = results.iter().filter_map(|(_, _, size, _)| *size).sum();
    let successful = results.iter().filter(|(_, ok, _, _)| *ok).count();
    let failed = results.iter().filter(|(_, ok, _, _)| !*ok).count();

    eprintln!();

    if verbose {
        #[derive(Tabled)]
        struct BackupResult {
            #[tabled(rename = "App")]
            app: String,
            #[tabled(rename = "Status")]
            status: String,
            #[tabled(rename = "Size")]
            size: String,
        }

        let table_data: Vec<BackupResult> = results
            .iter()
            .map(|(app, ok, size, err)| BackupResult {
                app: app.to_string(),
                status: if *ok {
                    "✓".to_string()
                } else {
                    format!("✗ {}", err.as_ref().unwrap())
                },
                size: size.map(output::format_size).unwrap_or_default(),
            })
            .collect();

        output::print_table(&table_data);
        eprintln!();
    }

    if failed == 0 {
        eprintln!(
            "Backed up {} app{} ({}) in {}",
            successful,
            if successful == 1 { "" } else { "s" },
            output::format_size(total_size),
            output::format_duration(elapsed)
        );
    } else {
        eprintln!(
            "Backup completed with errors ({} of {} apps failed)",
            failed,
            successful + failed
        );
    }

    if verbose {
        eprintln!(
            "Location: {}/{}/",
            backup_dest.join(&host.name).display(),
            timestamp
        );
    }

    if failed > 0 {
        eyre::bail!("{} backup(s) failed", failed);
    }

    Ok(())
}

pub fn run_backup_list(
    host_filter: Option<String>,
    app_filter: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let backup_root = default_backup_dir();

    if !backup_root.exists() {
        output::info("No backups found. Backup directory does not exist:");
        eprintln!("  {}", backup_root.display());
        return Ok(());
    }

    let backups = discover_backups(&backup_root, host_filter.as_deref(), app_filter.as_deref())?;

    if backups.is_empty() {
        output::info("No backups found");
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

#[derive(Tabled)]
struct BackupDisplay {
    #[tabled(rename = "HOST")]
    host: String,
    #[tabled(rename = "APP")]
    app: String,
    #[tabled(rename = "TIMESTAMP")]
    timestamp: String,
    #[tabled(rename = "SIZE")]
    size: String,
}

impl From<&BackupEntry> for BackupDisplay {
    fn from(entry: &BackupEntry) -> Self {
        Self {
            host: entry.host.clone(),
            app: entry.app.clone(),
            timestamp: entry.timestamp.clone(),
            size: output::format_size(entry.size_bytes),
        }
    }
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

        for timestamp_entry in fs::read_dir(host_entry.path())? {
            let timestamp_entry = timestamp_entry?;
            let timestamp_path = timestamp_entry.path();

            if timestamp_path.is_symlink() {
                continue;
            }

            if !timestamp_path.is_dir() {
                continue;
            }

            let timestamp = timestamp_entry.file_name().to_string_lossy().to_string();

            if !timestamp.contains('_') || !timestamp.starts_with("20") {
                continue;
            }

            for app_entry in fs::read_dir(timestamp_path)? {
                let app_entry = app_entry?;
                let app_path = app_entry.path();

                if !app_path.is_dir() {
                    continue;
                }

                let app_name = app_entry.file_name().to_string_lossy().to_string();

                if let Some(filter) = app_filter
                    && app_name != filter
                {
                    continue;
                }

                let size_bytes = calculate_dir_size(&app_path)?;

                backups.push(BackupEntry {
                    host: host_name.clone(),
                    app: app_name.clone(),
                    timestamp: timestamp.clone(),
                    path: app_path,
                    size_bytes,
                });
            }
        }
    }

    backups.sort_by(|a, b| {
        a.host
            .cmp(&b.host)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
            .then_with(|| a.app.cmp(&b.app))
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
    let display_backups: Vec<BackupDisplay> = backups.iter().map(BackupDisplay::from).collect();
    output::print_table(&display_backups);
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

pub fn run_backup_restore(opts: RestoreOptions) -> Result<()> {
    let host = get_host_or_select(opts.host_arg)?;
    let backup_root = default_backup_dir();

    let (source_host_name, is_cross_host) = match opts.from_host_arg {
        Some(ref from_host) => (from_host.clone(), from_host != &host.name),
        None => (host.name.clone(), false),
    };

    let host_backup_dir = backup_root.join(&source_host_name);

    let ssh_key_path = resolve_ssh_key_path(&host, opts.ssh_key)?;
    eprintln!("Using SSH key: {}", ssh_key_path.display());

    if !host_backup_dir.exists() {
        eyre::bail!("No backups found for host: {}", source_host_name);
    }

    let app_names = opts.apps.unwrap_or_else(|| {
        vec![
            "baikal".to_string(),
            "freshrss".to_string(),
            "navidrome".to_string(),
            "calibre".to_string(),
            "webdav".to_string(),
            "yourls".to_string(),
        ]
    });

    let mut restore_plan = Vec::new();

    for app_name in &app_names {
        let backup_path = if opts.backup_id == "latest" {
            let mut timestamps: Vec<_> = fs::read_dir(&host_backup_dir)?
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .filter(|e| !e.path().is_symlink())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.contains('_') && name.starts_with("20")
                })
                .collect();

            timestamps.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

            let latest_timestamp = timestamps.first();

            if let Some(timestamp_entry) = latest_timestamp {
                let app_path = timestamp_entry.path().join(app_name);
                if !app_path.exists() {
                    eprintln!(
                        "⚠ No backup found for {} in latest backup, skipping",
                        app_name
                    );
                    continue;
                }
                app_path
            } else {
                eprintln!("⚠ No backups found for {}, skipping", app_name);
                continue;
            }
        } else {
            let backup_path = host_backup_dir.join(&opts.backup_id).join(app_name);
            if !backup_path.exists() {
                eprintln!(
                    "⚠ Backup {} not found for {}, skipping",
                    opts.backup_id, app_name
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

    let total_backup_size: u64 = restore_plan
        .iter()
        .map(|(_, path)| calculate_dir_size(path).unwrap_or(0))
        .sum();

    if is_cross_host {
        validate_cross_host_restore(&host, &ssh_key_path, &app_names, total_backup_size)?;
    }

    eprintln!("\n=== Restore Plan ===");
    if is_cross_host {
        eprintln!("Source: {} (backup: {})", source_host_name, opts.backup_id);
        eprintln!("Target: {} ({}:{})", host.name, host.address, host.port);
        eprintln!("\n⚠  CROSS-HOST RESTORE WARNING");
        eprintln!(
            "   This will restore data from '{}' to '{}'",
            source_host_name, host.name
        );
        eprintln!("   Existing data on '{}' will be OVERWRITTEN", host.name);
    } else {
        eprintln!("Host: {}", host.name);
        eprintln!("Backup ID: {}", opts.backup_id);
    }
    eprintln!("\nApps to restore:");
    for (app, path) in &restore_plan {
        eprintln!("  - {:<12} from {}", app, path.display());
    }

    if opts.dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(());
    }

    if is_cross_host && !opts.yes {
        eprintln!("\n⚠  DANGER: Cross-host restore requires explicit confirmation");
        eprintln!("   Type the target host name '{}' to confirm:", host.name);

        let confirmation: String = dialoguer::Input::new()
            .with_prompt("Target host name")
            .interact_text()?;

        if confirmation.trim() != host.name {
            eprintln!("✗ Confirmation failed. Restore cancelled");
            return Ok(());
        }
    } else if !opts.yes {
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

    if is_cross_host && opts.yes {
        eprintln!("\n⚠  Cross-host restore with --yes flag");
        eprintln!("   Waiting 3 seconds (press Ctrl+C to cancel)...");
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    if is_cross_host && !opts.dry_run {
        eprintln!("\n--- Creating Emergency Backup ---");
        eprintln!(
            "  Backing up current state of '{}' before cross-host restore",
            host.name
        );

        let emergency_timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let emergency_backup_name = format!("pre-migration-{}", emergency_timestamp);

        match run_backup_create(
            Some(host.name.clone()),
            Some(app_names.clone()),
            Some(backup_root.clone()),
            Some(ssh_key_path.clone()),
            false,
            false,
            false,
        ) {
            Ok(_) => {
                eprintln!("  ✓ Emergency backup created: {}", emergency_backup_name);
                eprintln!(
                    "    Location: {}/{}/{}/",
                    backup_root.display(),
                    host.name,
                    emergency_timestamp
                );
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to create emergency backup: {}", e);
                eprintln!("    Continue without emergency backup? This is DANGEROUS!");

                if !dialoguer::Confirm::new()
                    .with_prompt("Continue without emergency backup?")
                    .default(false)
                    .interact()?
                {
                    eprintln!("Restore cancelled");
                    return Ok(());
                }
            }
        }
    }

    let phase_label = if opts.skip_playbook_unsafe || opts.dry_run {
        ""
    } else {
        "[1/2] "
    };
    eprintln!("\n{}Starting restore...", phase_label);

    for (app_name, backup_path) in restore_plan {
        restore_app(&host, &app_name, &backup_path, &ssh_key_path)?;
    }

    eprintln!("\n✓ All restores completed successfully");

    if !opts.skip_playbook_unsafe && !opts.dry_run {
        eprintln!("\n[2/2] Running Ansible playbooks to fix permissions...");

        let project_root = crate::services::inventory::find_project_root();
        let apps_playbook = project_root.join("ansible/playbooks/apps.yml");

        if !apps_playbook.exists() {
            eprintln!("⚠ Ansible playbook not found: {}", apps_playbook.display());
            eprintln!("  Services may fail due to incorrect file ownership!");
            eprintln!("  Run manually: cd ansible && ansible-playbook playbooks/apps.yml");
        } else {
            let tags: Vec<String> = app_names.iter().map(|s| s.to_string()).collect();

            let inventory_host = crate::services::ansible_runner::InventoryHost {
                name: host.name.clone(),
                address: host.address.clone(),
                port: host.port,
                user: host.user.clone(),
            };

            match crate::services::ansible_runner::run_playbook(
                &apps_playbook,
                &inventory_host,
                false,
                Some(&tags),
                None,
                false,
                false,
            ) {
                Ok(result) if result.success => {
                    eprintln!("✓ Ansible playbooks completed successfully");
                    eprintln!("  File permissions have been corrected");
                }
                Ok(result) => {
                    eprintln!(
                        "⚠ Ansible playbook failed (exit code: {})",
                        result.exit_code
                    );
                    eprintln!("  Services may fail due to incorrect file ownership!");
                    eprintln!(
                        "  Fix manually: cd ansible && ansible-playbook playbooks/apps.yml --tags {}",
                        tags.join(",")
                    );
                }
                Err(e) => {
                    eprintln!("⚠ Failed to run Ansible playbook: {}", e);
                    eprintln!("  Services may fail due to incorrect file ownership!");
                    eprintln!(
                        "  Fix manually: cd ansible && ansible-playbook playbooks/apps.yml --tags {}",
                        tags.join(",")
                    );
                }
            }
        }
    } else if opts.skip_playbook_unsafe && !opts.dry_run {
        eprintln!("\n⚠️  WARNING: Skipped Ansible playbooks (--skip-playbook-unsafe)");
        eprintln!("⚠️  Services WILL fail until you run:");
        eprintln!(
            "     cd ansible && ansible-playbook playbooks/apps.yml --tags {}",
            app_names.join(",")
        );
    }

    if is_cross_host {
        eprintln!("\n=== Post-Restore Actions Required ===");
        eprintln!("  Cross-host restore completed. Manual verification needed:\n");
        let all_services: Vec<&str> = app_names
            .iter()
            .filter_map(|name| AppBackupConfig::by_name(name, false))
            .flat_map(|cfg| cfg.systemd_services)
            .collect();
        if !all_services.is_empty() {
            eprintln!("  1. Verify services are running:");
            eprintln!(
                "     ssh {}@{} 'systemctl status {}'",
                host.user,
                host.address,
                all_services.join(" ")
            );
        }
        eprintln!("\n  2. Check service logs for errors:");
        for app_name in &app_names {
            if let Some(cfg) = AppBackupConfig::by_name(app_name, false) {
                for service in &cfg.systemd_services {
                    eprintln!(
                        "     ssh {}@{} 'journalctl -u {} --since \"5 minutes ago\" | grep -i error'",
                        host.user, host.address, service
                    );
                }
            }
        }
        eprintln!("\n  3. Update DNS records if hostnames changed");
        eprintln!("\n  4. Verify SSL certificates are valid for new domain\n");

        eprintln!("  ⚠  App-specific notes:");
        for app_name in &app_names {
            match app_name.as_str() {
                "navidrome" => {
                    eprintln!("     - Navidrome: May need to rescan music library");
                    eprintln!("       Fix: Trigger rescan from web UI or restart service");
                }
                "freshrss" => {
                    eprintln!(
                        "     - FreshRSS: Database paths should be fine, but verify feeds update"
                    );
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn restore_app(host: &Host, app_name: &str, backup_path: &Path, ssh_key: &Path) -> Result<()> {
    eprintln!("\n--- Restoring {} ---", app_name);

    let config = AppBackupConfig::by_name(app_name, false)
        .ok_or_else(|| eyre::eyre!("Unknown app: {}", app_name))?;

    let mut stopped_services: Vec<&str> = Vec::new();
    for service in &config.systemd_services {
        eprintln!("  Stopping service: {}", service);
        if let Err(e) = remote_systemctl(host, ssh_key, "stop", service) {
            for previously_stopped in &stopped_services {
                let _ = remote_systemctl(host, ssh_key, "start", previously_stopped);
            }
            return Err(e).wrap_err_with(|| format!("Failed to stop service {}", service));
        }
        stopped_services.push(service);
    }

    let restore_result = (|| -> Result<()> {
        for remote_path in &config.paths {
            eprintln!("  Restoring to: {}", remote_path);
            rsync_to_remote(host, ssh_key, backup_path, remote_path)?;
        }

        if let Some((user, group)) = config.owner {
            eprintln!("  Setting ownership to {}:{}", user, group);
            for remote_path in &config.paths {
                set_remote_ownership(host, ssh_key, remote_path, user, group)?;
            }
        }

        if let Some(db) = &config.db {
            let local_dump = backup_path.join("db.dump");
            if local_dump.exists() {
                eprintln!("  Restoring database: {}", db.db_name);
                scp_to_remote(host, ssh_key, &local_dump, db.remote_dump_path)?;
                remote_ssh_command(host, ssh_key, &format!("chmod 644 {}", db.remote_dump_path))?;
                let pg_result = remote_pg_restore(host, ssh_key, db);
                remote_pg_dump_cleanup(host, ssh_key, db);
                pg_result?;

                if config.name == "paperless" {
                    eprintln!("  Running database migrations");
                    let migrate_cmd = "sudo -u paperless /opt/paperless/src/venv/bin/python3 manage.py migrate --no-input";
                    let migrate_result = remote_ssh_command(
                        host,
                        ssh_key,
                        &format!(
                            "cd /opt/paperless/src && PAPERLESS_CONFIGURATION_PATH=/opt/paperless/paperless.conf {}",
                            migrate_cmd
                        ),
                    );
                    if let Err(e) = migrate_result {
                        output::warn(&format!("Migration warning: {}", e));
                    }
                }
            } else {
                output::warn(&format!(
                    "No database dump found at {}, skipping db restore",
                    local_dump.display()
                ));
            }
        }

        Ok(())
    })();

    let mut start_failures: Vec<String> = Vec::new();
    for service in &config.systemd_services {
        eprintln!("  Starting service: {}", service);
        if let Err(e) = remote_systemctl(host, ssh_key, "start", service) {
            start_failures.push(format!("{}: {}", service, e));
        }
    }

    match restore_result {
        Ok(()) => {
            if !start_failures.is_empty() {
                eyre::bail!(
                    "Restore of {} succeeded but failed to restart services:\n  {}",
                    app_name,
                    start_failures.join("\n  ")
                );
            }
        }
        Err(e) => {
            if !start_failures.is_empty() {
                eyre::bail!(
                    "Restore of {} failed: {}\nAdditionally, failed to restart services:\n  {}",
                    app_name,
                    e,
                    start_failures.join("\n  ")
                );
            }
            return Err(e);
        }
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

    let session = SshSession::new(host, ssh_key);
    let _ = Command::new("ssh")
        .args(session.ssh_args())
        .arg("sudo")
        .arg("mkdir")
        .arg("-p")
        .arg(parent_dir)
        .status();

    let mut cmd = Command::new("rsync");
    cmd.arg("-az")
        .arg("--delete")
        .arg("--rsync-path=sudo rsync")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    for pattern in RSYNC_EXCLUDES {
        cmd.arg(format!("--exclude={}", pattern));
    }

    cmd.arg("-e")
        .arg(session.rsync_e_arg())
        .arg(format!("{}/", local_source.display()))
        .arg(format!("{}@{}:{}", host.user, host.address, remote_path));

    let status = cmd.status().wrap_err("Failed to execute rsync")?;

    if !status.success() {
        eyre::bail!("rsync failed for {}", remote_path);
    }

    Ok(())
}

pub fn run_export_opml(
    host_arg: Option<String>,
    output: PathBuf,
    ssh_key: Option<PathBuf>,
    user: String,
) -> Result<()> {
    let host = get_host_or_select(host_arg)?;
    let ssh_key_path = resolve_ssh_key_path(&host, ssh_key)?;
    eprintln!("Using SSH key: {}", ssh_key_path.display());

    eprintln!("Exporting OPML from FreshRSS");
    eprintln!("  Host: {}", host.name);
    eprintln!("  User: {}", user);
    eprintln!("  Output: {}", output.display());

    let remote_cmd = format!(
        "cd /opt/freshrss && sudo -u freshrss ./cli/export-opml-for-user.php --user {}",
        user
    );

    let session = SshSession::new(&host, &ssh_key_path);
    let opml_output = session.run(&remote_cmd)?;

    if !opml_output.status.success() {
        let stderr = String::from_utf8_lossy(&opml_output.stderr);
        eyre::bail!("OPML export failed: {}", stderr);
    }

    fs::write(&output, &opml_output.stdout)
        .wrap_err_with(|| format!("Failed to write OPML to {}", output.display()))?;

    eprintln!("✓ OPML exported successfully");
    eprintln!("  Saved to: {}", output.display());

    Ok(())
}

pub fn run_import_opml(
    host_arg: Option<String>,
    input: PathBuf,
    ssh_key: Option<PathBuf>,
    user: String,
) -> Result<()> {
    let host = get_host_or_select(host_arg)?;
    let ssh_key_path = resolve_ssh_key_path(&host, ssh_key)?;
    eprintln!("Using SSH key: {}", ssh_key_path.display());

    if !input.exists() {
        eyre::bail!("OPML file not found: {}", input.display());
    }

    eprintln!("Importing OPML to FreshRSS");
    eprintln!("  Host: {}", host.name);
    eprintln!("  User: {}", user);
    eprintln!("  Input: {}", input.display());

    let remote_opml_path = format!("/tmp/freshrss_import_{}.opml", user);

    eprintln!("  Uploading OPML file...");
    let session = SshSession::new(&host, &ssh_key_path);
    session
        .scp_to(&input, &remote_opml_path)
        .wrap_err("Failed to upload OPML file")?;

    eprintln!("  Importing feeds...");
    let import_cmd = format!(
        "cd /opt/freshrss && sudo -u freshrss ./cli/import-for-user.php --user {} --filename {} && rm {}",
        user, remote_opml_path, remote_opml_path
    );

    let import_output = session
        .run(&import_cmd)
        .wrap_err("Failed to execute import command")?;

    if !import_output.status.success() {
        let stderr = String::from_utf8_lossy(&import_output.stderr);
        eyre::bail!("OPML import failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&import_output.stdout);
    eprintln!("{}", stdout);

    eprintln!("✓ OPML imported successfully");

    Ok(())
}

fn load_restic_config() -> Result<(String, String)> {
    let config = UserConfig::load()?;
    let missing = config.validate_required(&["restic_repository", "restic_password"]);
    if !missing.is_empty() {
        eyre::bail!(
            "Missing restic config: {}. Set with `auberge config set <key> <value>`",
            missing.join(", ")
        );
    }
    Ok((
        config.get("restic_repository").unwrap(),
        config.get("restic_password").unwrap(),
    ))
}

pub fn run_backup_push(host_filter: Option<String>, backup_id: Option<String>) -> Result<()> {
    let (restic_repo, restic_password) = load_restic_config()?;

    let backup_root = default_backup_dir();
    if !backup_root.exists() {
        eyre::bail!("No backups found. Run `auberge backup create` first.");
    }

    let backup_dir =
        resolve_backup_dir(&backup_root, host_filter.as_deref(), backup_id.as_deref())?;

    output::info(&format!("Pushing {} to restic", backup_dir.display()));

    let spinner = output::spinner("Checking restic repository");
    let snapshots_check = Command::new("restic")
        .arg("snapshots")
        .arg("--json")
        .env("RESTIC_REPOSITORY", &restic_repo)
        .env("RESTIC_PASSWORD", &restic_password)
        .output();

    let needs_init = match snapshots_check {
        Ok(output) if output.status.success() => false,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Is there a repository at the following location")
                || stderr.contains("unable to open config file")
            {
                true
            } else {
                eyre::bail!("restic snapshots failed: {}", stderr.trim());
            }
        }
        Err(_) => eyre::bail!("restic not found. Install restic: https://restic.net"),
    };

    if needs_init {
        spinner.set_message("Initializing restic repository".to_string());
        let init_output = Command::new("restic")
            .arg("init")
            .env("RESTIC_REPOSITORY", &restic_repo)
            .env("RESTIC_PASSWORD", &restic_password)
            .output()
            .wrap_err("Failed to initialize restic repository")?;

        if !init_output.status.success() {
            let stderr = String::from_utf8_lossy(&init_output.stderr);
            eyre::bail!("Failed to initialize restic repository: {}", stderr.trim());
        }
    }

    spinner.set_message(format!("Backing up {}", backup_dir.display()));
    let backup_output = Command::new("restic")
        .arg("backup")
        .arg(&backup_dir)
        .env("RESTIC_REPOSITORY", &restic_repo)
        .env("RESTIC_PASSWORD", &restic_password)
        .output()
        .wrap_err("Failed to run restic backup")?;

    spinner.finish_and_clear();

    if !backup_output.status.success() {
        let stderr = String::from_utf8_lossy(&backup_output.stderr);
        eyre::bail!("restic backup failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&backup_output.stdout);
    let snapshot_id = stdout
        .lines()
        .find(|line| line.contains("snapshot") && line.contains("saved"))
        .unwrap_or("backup completed");

    output::success(&format!("Push complete: {}", snapshot_id.trim()));

    Ok(())
}

pub fn run_backup_prune(dry_run: bool) -> Result<()> {
    let (restic_repo, restic_password) = load_restic_config()?;

    let spinner = output::spinner("Pruning restic snapshots");

    let mut cmd = Command::new("restic");
    cmd.arg("forget")
        .arg("--keep-daily")
        .arg("7")
        .arg("--keep-weekly")
        .arg("4")
        .arg("--keep-monthly")
        .arg("12")
        .arg("--prune")
        .env("RESTIC_REPOSITORY", &restic_repo)
        .env("RESTIC_PASSWORD", &restic_password);

    if dry_run {
        cmd.arg("--dry-run");
    }

    let output = cmd.output().wrap_err("Failed to run restic forget")?;

    spinner.finish_and_clear();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eyre::bail!("restic prune failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        eprintln!("{}", stdout.trim());
    }

    if dry_run {
        output::info("Dry run completed (no changes made)");
    } else {
        output::success("Prune complete");
    }

    Ok(())
}

fn resolve_backup_dir(
    backup_root: &Path,
    host_filter: Option<&str>,
    backup_id: Option<&str>,
) -> Result<PathBuf> {
    let host_dir = match host_filter {
        Some(host) => {
            let dir = backup_root.join(host);
            if !dir.exists() {
                eyre::bail!("No backups found for host: {}", host);
            }
            dir
        }
        None => {
            let mut hosts: Vec<_> = fs::read_dir(backup_root)?
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .collect();
            if hosts.is_empty() {
                eyre::bail!("No backups found");
            }
            if hosts.len() == 1 {
                hosts.remove(0).path()
            } else {
                let host_names: Vec<String> = hosts
                    .iter()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                let selection = select_item(
                    &host_names,
                    |h: &String| h.clone(),
                    "Select host backup to push",
                )?
                .ok_or_else(|| eyre::eyre!("No host selected"))?;
                backup_root.join(&selection)
            }
        }
    };

    match backup_id {
        Some(id) => {
            let dir = host_dir.join(id);
            if !dir.exists() {
                eyre::bail!("Backup not found: {}", dir.display());
            }
            Ok(dir)
        }
        None => {
            let mut timestamps: Vec<_> = fs::read_dir(&host_dir)?
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir() && !e.path().is_symlink())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.contains('_') && name.starts_with("20")
                })
                .collect();

            timestamps.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

            timestamps
                .first()
                .map(|e| e.path())
                .ok_or_else(|| eyre::eyre!("No backup timestamps found in {}", host_dir.display()))
        }
    }
}

fn get_host_or_select(host_arg: Option<String>) -> Result<Host> {
    match host_arg {
        Some(name) => HostManager::get_host(&name),
        None => {
            let hosts = HostManager::load_hosts()?;
            select_item(
                &hosts,
                |h: &Host| format!("{} ({}:{})", h.name, h.address, h.port),
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

fn backup_app(
    host: &Host,
    config: &AppBackupConfig,
    backup_dest: &Path,
    ssh_key: &Path,
    timestamp: &str,
    verbose: bool,
) -> Result<u64> {
    let spinner = output::spinner(&format!("Backing up {}", config.name));
    let app_backup_dir = backup_dest
        .join(&host.name)
        .join(timestamp)
        .join(config.name);

    fs::create_dir_all(&app_backup_dir).wrap_err_with(|| {
        format!(
            "Failed to create backup directory: {}",
            app_backup_dir.display()
        )
    })?;

    let mut stopped_services: Vec<&str> = Vec::new();
    if !config.systemd_services.is_empty() {
        spinner.set_message(format!("Backing up {} (stopping services)", config.name));
        for service in &config.systemd_services {
            if let Err(e) = remote_systemctl(host, ssh_key, "stop", service) {
                for previously_stopped in &stopped_services {
                    let _ = remote_systemctl(host, ssh_key, "start", previously_stopped);
                }
                return Err(e).wrap_err_with(|| format!("Failed to stop service {}", service));
            }
            stopped_services.push(service);
        }
    }

    if let Some(db) = &config.db {
        spinner.set_message(format!("Backing up {} (dumping database)", config.name));
        if let Err(e) = remote_pg_dump(host, ssh_key, db) {
            remote_pg_dump_cleanup(host, ssh_key, db);
            for service in &stopped_services {
                let _ = remote_systemctl(host, ssh_key, "start", service);
            }
            return Err(e).wrap_err("pg_dump failed");
        }
    }

    spinner.set_message(format!("Backing up {} (copying files)", config.name));
    let rsync_result = (|| -> Result<()> {
        for path in &config.paths {
            rsync_from_remote(host, ssh_key, path, &app_backup_dir)?;
        }
        if let Some(db) = &config.db {
            scp_from_remote(
                host,
                ssh_key,
                db.remote_dump_path,
                &app_backup_dir.join("db.dump"),
            )?;
        }
        Ok(())
    })();

    if let Some(db) = &config.db {
        remote_pg_dump_cleanup(host, ssh_key, db);
    }

    let mut start_failures: Vec<String> = Vec::new();
    if !config.systemd_services.is_empty() {
        spinner.set_message(format!("Backing up {} (starting services)", config.name));
        for service in &config.systemd_services {
            if let Err(e) = remote_systemctl(host, ssh_key, "start", service) {
                start_failures.push(format!("{}: {}", service, e));
            }
        }
    }

    match rsync_result {
        Ok(()) => {
            if !start_failures.is_empty() {
                eyre::bail!(
                    "Backup of {} succeeded but failed to restart services:\n  {}",
                    config.name,
                    start_failures.join("\n  ")
                );
            }
        }
        Err(e) => {
            if !start_failures.is_empty() {
                eyre::bail!(
                    "Backup of {} failed during file copy: {}\nAdditionally, failed to restart services:\n  {}",
                    config.name,
                    e,
                    start_failures.join("\n  ")
                );
            }
            return Err(e);
        }
    }

    let backup_size = calculate_dir_size(&app_backup_dir)?;

    if verbose {
        spinner.finish_and_clear();
    } else {
        spinner.finish_with_message(format!(
            "  ✓ {} ({})",
            config.name,
            output::format_size(backup_size)
        ));
    }

    Ok(backup_size)
}

fn resolve_ssh_key_path(host: &Host, override_key: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(key_path) = override_key {
        if !key_path.exists() {
            eyre::bail!(
                "Specified SSH key not found: {}\nCheck the path and try again",
                key_path.display()
            );
        }

        validate_key_file(&key_path)?;
        return Ok(key_path);
    }

    if let Some(ref configured_key) = host.ssh_key {
        let key_path = PathBuf::from(shellexpand::tilde(configured_key).as_ref());
        if key_path.exists() {
            validate_key_file(&key_path)?;
            return Ok(key_path);
        }
        eprintln!(
            "⚠ Warning: Configured SSH key not found: {}",
            key_path.display()
        );
        eprintln!("  Falling back to default key derivation");
    }

    let ssh_key = dirs::home_dir()
        .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
        .join(format!(".ssh/identities/{}_{}", host.user, host.name));

    if !ssh_key.exists() {
        eyre::bail!(
            "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}' or configure with 'auberge host edit {}'",
            ssh_key.display(),
            host.name,
            host.user,
            host.name
        );
    }

    Ok(ssh_key)
}

fn validate_key_file(key_path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(key_path)
        .wrap_err_with(|| format!("Cannot read SSH key: {}", key_path.display()))?;

    if !metadata.is_file() {
        eyre::bail!("SSH key path is not a file: {}", key_path.display());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = metadata.permissions();
        let mode = perms.mode() & 0o777;
        if mode & 0o077 != 0 {
            eprintln!(
                "⚠ Warning: SSH key has overly permissive permissions: {:o}",
                mode
            );
            eprintln!("  Consider running: chmod 600 {}", key_path.display());
        }
    }

    Ok(())
}

fn remote_ssh_command(host: &Host, ssh_key: &Path, command: &str) -> Result<std::process::Output> {
    let output = SshSession::new(host, ssh_key).run(command)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eyre::bail!("Remote command failed: {}", stderr.trim());
    }

    Ok(output)
}

fn remote_ssh_command_raw(
    host: &Host,
    ssh_key: &Path,
    command: &str,
) -> Result<std::process::Output> {
    SshSession::new(host, ssh_key).run(command)
}

fn remote_pg_dump(host: &Host, ssh_key: &Path, db: &DbBackupConfig) -> Result<()> {
    let cmd = format!(
        "sudo -u postgres pg_dump -Fc {} > {}",
        db.db_name, db.remote_dump_path
    );
    remote_ssh_command(host, ssh_key, &cmd)?;
    Ok(())
}

fn remote_pg_dump_cleanup(host: &Host, ssh_key: &Path, db: &DbBackupConfig) {
    let cmd = format!("rm -f {}", db.remote_dump_path);
    let _ = remote_ssh_command(host, ssh_key, &cmd);
}

fn remote_pg_restore(host: &Host, ssh_key: &Path, db: &DbBackupConfig) -> Result<()> {
    let cmd = format!(
        "sudo -u postgres pg_restore --clean --if-exists -d {} {} 2>&1",
        db.db_name, db.remote_dump_path
    );
    let output = remote_ssh_command_raw(host, ssh_key, &cmd)?;

    if !output.status.success() {
        let combined_output = String::from_utf8_lossy(&output.stdout);
        let ssh_stderr = String::from_utf8_lossy(&output.stderr);

        if !ssh_stderr.trim().is_empty() {
            eyre::bail!("pg_restore SSH error: {}", ssh_stderr.trim());
        }

        if combined_output.trim().is_empty() {
            eyre::bail!(
                "pg_restore failed with exit code {}",
                output.status.code().unwrap_or(-1)
            );
        }

        let warnings_only = combined_output.lines().all(|line| {
            let trimmed = line.trim().to_lowercase();
            trimmed.is_empty()
                || trimmed.contains("warning")
                || trimmed.starts_with("pg_restore: warning")
        });
        if !warnings_only {
            eyre::bail!("pg_restore failed: {}", combined_output.trim());
        }
    }

    Ok(())
}

fn scp_from_remote(
    host: &Host,
    ssh_key: &Path,
    remote_path: &str,
    local_path: &Path,
) -> Result<()> {
    SshSession::new(host, ssh_key).scp_from(remote_path, local_path)
}

fn scp_to_remote(host: &Host, ssh_key: &Path, local_path: &Path, remote_path: &str) -> Result<()> {
    SshSession::new(host, ssh_key).scp_to(local_path, remote_path)
}

fn remote_systemctl(host: &Host, ssh_key: &Path, action: &str, service: &str) -> Result<()> {
    SshSession::new(host, ssh_key).systemctl(action, service)
}

fn set_remote_ownership(
    host: &Host,
    ssh_key: &Path,
    remote_path: &str,
    user: &str,
    group: &str,
) -> Result<()> {
    let session = SshSession::new(host, ssh_key);
    let status = Command::new("ssh")
        .args(session.ssh_args())
        .arg("sudo")
        .arg("chown")
        .arg("-R")
        .arg(format!("{}:{}", user, group))
        .arg(remote_path)
        .status()
        .wrap_err_with(|| {
            format!(
                "Failed to set ownership of {} to {}:{}",
                remote_path, user, group
            )
        })?;

    if !status.success() {
        eyre::bail!("chown -R {}:{} {} failed", user, group, remote_path);
    }

    Ok(())
}

fn rsync_from_remote(
    host: &Host,
    ssh_key: &Path,
    remote_path: &str,
    local_dest: &Path,
) -> Result<()> {
    let session = SshSession::new(host, ssh_key);
    let mut cmd = Command::new("rsync");
    cmd.arg("-az")
        .arg("--relative")
        .arg("--rsync-path=sudo rsync")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    for pattern in RSYNC_EXCLUDES {
        cmd.arg(format!("--exclude={}", pattern));
    }

    cmd.arg("-e")
        .arg(session.rsync_e_arg())
        .arg(format!("{}@{}:{}", host.user, host.address, remote_path))
        .arg(local_dest);

    let status = cmd.status().wrap_err("Failed to execute rsync")?;

    if !status.success() {
        eyre::bail!("rsync failed for {}", remote_path);
    }

    Ok(())
}

fn check_remote_service_exists(host: &Host, ssh_key: &Path, service: &str) -> Result<bool> {
    let output = SshSession::new(host, ssh_key).run_raw(&[
        "systemctl",
        "list-unit-files",
        &format!("{}.service", service),
    ])?;
    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).contains(&format!("{}.service", service)))
}

fn check_remote_disk_space(host: &Host, ssh_key: &Path, path: &str) -> Result<u64> {
    let output = SshSession::new(host, ssh_key)
        .run(&format!("df --output=avail {} | tail -1", path))
        .wrap_err("Failed to check disk space")?;

    if !output.status.success() {
        eyre::bail!("Failed to check disk space on remote host");
    }

    let kb_available = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .wrap_err("Failed to parse disk space output")?;

    Ok(kb_available * 1024)
}

fn validate_cross_host_restore(
    host: &Host,
    ssh_key: &Path,
    apps: &[String],
    backup_size_bytes: u64,
) -> Result<()> {
    eprintln!("\n--- Pre-flight Validation ---");

    eprintln!("  Checking SSH connectivity...");
    let ssh_test = Command::new("ssh")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-i")
        .arg(ssh_key)
        .arg("-p")
        .arg(host.port.to_string())
        .arg(format!("{}@{}", host.user, host.address))
        .arg("echo")
        .arg("ok")
        .output();

    match ssh_test {
        Ok(output) if output.status.success() => {
            eprintln!("    ✓ SSH connection successful");
        }
        _ => {
            eyre::bail!(
                "Cannot connect to target host {}:{}. Check SSH key and network connectivity",
                host.address,
                host.port
            );
        }
    }

    eprintln!("  Checking services on target...");
    for app in apps {
        let config = AppBackupConfig::by_name(app, false);
        if let Some(cfg) = config {
            for service in &cfg.systemd_services {
                match check_remote_service_exists(host, ssh_key, service) {
                    Ok(true) => {
                        eprintln!("    ✓ {} service exists", service);
                    }
                    Ok(false) => {
                        eprintln!("    ⚠ {} service not found on target", service);
                        eprintln!(
                            "      Run 'auberge ansible run --host {}' to install services",
                            host.name
                        );
                        eyre::bail!("Required service {} not found on target host", service);
                    }
                    Err(e) => {
                        eprintln!("    ⚠ Failed to check {}: {}", service, e);
                    }
                }
            }
        }
    }

    eprintln!("  Checking disk space...");
    match check_remote_disk_space(host, ssh_key, "/") {
        Ok(available_bytes) => {
            let required_bytes = (backup_size_bytes as f64 * 1.2) as u64;
            eprintln!(
                "    Available: {}, Required: {} (with 20% buffer)",
                format_size(available_bytes),
                format_size(required_bytes)
            );

            if available_bytes < required_bytes {
                eyre::bail!(
                    "Insufficient disk space: need {}, have {}",
                    format_size(required_bytes),
                    format_size(available_bytes)
                );
            }
            eprintln!("    ✓ Sufficient disk space available");
        }
        Err(e) => {
            eprintln!("    ⚠ Failed to check disk space: {}", e);
            eprintln!("    Proceeding anyway (use at your own risk)");
        }
    }

    eprintln!("✓ Pre-flight validation completed\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paperless_has_db_config() {
        let config = AppBackupConfig::paperless();
        assert!(config.db.is_some());
    }

    #[test]
    fn test_other_apps_have_no_db_config() {
        assert!(AppBackupConfig::baikal().db.is_none());
        assert!(AppBackupConfig::freshrss().db.is_none());
        assert!(AppBackupConfig::navidrome(false).db.is_none());
        assert!(AppBackupConfig::navidrome(true).db.is_none());
        assert!(AppBackupConfig::calibre().db.is_none());
        assert!(AppBackupConfig::webdav().db.is_none());
        assert!(AppBackupConfig::yourls().db.is_none());
    }

    #[test]
    fn test_db_backup_config_fields() {
        let config = AppBackupConfig::paperless();
        let db = config.db.unwrap();
        assert_eq!(db.db_name, "paperless");
        assert_eq!(db.remote_dump_path, "/tmp/paperless_db.dump");
    }

    #[test]
    fn test_push_variant_exists() {
        let _push = BackupCommands::Push {
            host: None,
            backup_id: None,
        };
    }

    #[test]
    fn test_prune_variant_exists() {
        let _prune = BackupCommands::Prune { dry_run: true };
    }

    #[test]
    fn test_all_apps_returns_seven() {
        let all = AppBackupConfig::all();
        assert_eq!(all.len(), 7);
    }

    #[test]
    fn test_by_name_unknown_returns_none() {
        assert!(AppBackupConfig::by_name("nonexistent", false).is_none());
    }

    #[test]
    fn test_navidrome_music_paths() {
        let without = AppBackupConfig::navidrome(false);
        assert!(!without.paths.contains(&"/srv/music"));

        let with = AppBackupConfig::navidrome(true);
        assert!(with.paths.contains(&"/srv/music"));
    }

    #[test]
    fn test_resolve_backup_dir_empty_root() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_backup_dir(tmp.path(), None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No backups found"));
    }

    #[test]
    fn test_resolve_backup_dir_single_host_auto_selects() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        let ts_dir = host_dir.join("2026-03-09_14-30-00");
        fs::create_dir(&ts_dir).unwrap();

        let result = resolve_backup_dir(tmp.path(), None, None).unwrap();
        assert_eq!(result, ts_dir);
    }

    #[test]
    fn test_resolve_backup_dir_with_host_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let host_a = tmp.path().join("server-a");
        let host_b = tmp.path().join("server-b");
        fs::create_dir(&host_a).unwrap();
        fs::create_dir(&host_b).unwrap();
        let ts = host_b.join("2026-03-09_14-30-00");
        fs::create_dir(&ts).unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("server-b"), None).unwrap();
        assert_eq!(result, ts);
    }

    #[test]
    fn test_resolve_backup_dir_host_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_backup_dir(tmp.path(), Some("nonexistent"), None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No backups found for host")
        );
    }

    #[test]
    fn test_resolve_backup_dir_picks_latest_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        fs::create_dir(host_dir.join("2026-03-01_10-00-00")).unwrap();
        fs::create_dir(host_dir.join("2026-03-09_14-30-00")).unwrap();
        fs::create_dir(host_dir.join("2026-03-05_12-00-00")).unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("myserver"), None).unwrap();
        assert_eq!(result, host_dir.join("2026-03-09_14-30-00"));
    }

    #[test]
    fn test_resolve_backup_dir_excludes_symlinks_and_non_timestamp_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        let ts_dir = host_dir.join("2026-03-09_14-30-00");
        fs::create_dir(&ts_dir).unwrap();
        fs::create_dir(host_dir.join("not-a-timestamp")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&ts_dir, host_dir.join("latest")).unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("myserver"), None).unwrap();
        assert_eq!(result, ts_dir);
    }

    #[test]
    fn test_resolve_backup_dir_specific_backup_id() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        let ts = host_dir.join("2026-03-09_14-30-00");
        fs::create_dir(&ts).unwrap();

        let result =
            resolve_backup_dir(tmp.path(), Some("myserver"), Some("2026-03-09_14-30-00")).unwrap();
        assert_eq!(result, ts);
    }

    #[test]
    fn test_resolve_backup_dir_specific_backup_id_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("myserver"), Some("2026-01-01_00-00-00"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Backup not found"));
    }
}
