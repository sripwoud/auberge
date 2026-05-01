use crate::config::Config;
use crate::hosts::{Host, select_or_arg as hosts_select_or_arg};
use crate::output;
use crate::prompt::confirm;
use crate::services::backup::executor::RecipeExecutor;
use crate::services::backup::recipe::{
    assets_playbooks_dir, discover_backuppable_apps, load_app_recipe,
};
use crate::services::backup::ssh::LiveSshSession;
use crate::ssh_session::SshSession;
use chrono::Utc;
use clap::Subcommand;
use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tabled::Tabled;

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
            help = "Apps to backup (baikal,bichon,freshrss,headscale,navidrome,calibre,webdav,yourls,paperless). Default: all"
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
    },
    #[command(
        alias = "s",
        about = "Create backup, push to restic, prune, and clean up local staging"
    )]
    Sync {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to backup (baikal,bichon,freshrss,headscale,navidrome,calibre,webdav,yourls,paperless). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{user}_{host})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(long, help = "Include music files in Navidrome backup (large, slow)")]
        include_music: bool,
        #[arg(
            short = 'n',
            long,
            help = "Dry run (runs create in preview mode, skips push/prune/cleanup)"
        )]
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
            short = 'F',
            long,
            help = "Source host (for cross-host restore/migration)"
        )]
        from_host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to restore (baikal,bichon,freshrss,headscale,navidrome,calibre,webdav,yourls,paperless). Default: all"
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

#[derive(Debug, Clone)]
pub struct CreateOutcome {
    pub successful_apps: Vec<String>,
    pub failed_apps: Vec<(String, String)>,
    pub timestamp: String,
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

fn parameter_map(include_music: bool) -> HashMap<String, bool> {
    let mut params = HashMap::new();
    params.insert("include_music".to_string(), include_music);
    params
}

pub fn run_backup_create(
    host_arg: Option<String>,
    apps: Option<Vec<String>>,
    dest: Option<PathBuf>,
    ssh_key: Option<PathBuf>,
    include_music: bool,
    dry_run: bool,
) -> Result<CreateOutcome> {
    let host = get_host_or_select(host_arg)?;
    let backup_dest = dest.unwrap_or_else(default_backup_dir);

    let ssh_key_path = resolve_ssh_key_path(&host, ssh_key)?;

    if output::is_verbose() {
        output::info(&format!("SSH key: {}", ssh_key_path.display()));
        output::info(&format!(
            "Backing up to: {}",
            backup_dest.join(&host.name).display()
        ));
    } else {
        let short_dest = backup_dest
            .to_string_lossy()
            .replace(&std::env::var("HOME").unwrap_or_default(), "~");
        eprintln!("Backing up {} → {}", host.name, short_dest);
    }

    let playbooks_dir = assets_playbooks_dir()?;
    let app_names: Vec<String> = match apps {
        Some(names) => names
            .into_iter()
            .filter(|name| load_app_recipe(&playbooks_dir, name).is_ok())
            .collect(),
        None => discover_backuppable_apps(&playbooks_dir)?,
    };

    if app_names.is_empty() {
        eyre::bail!("No valid apps specified for backup");
    }

    if output::is_verbose() {
        output::info(&format!("Apps: {}", app_names.join(", ")));
    }

    let parameters = parameter_map(include_music);

    if dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(CreateOutcome {
            successful_apps: Vec::new(),
            failed_apps: Vec::new(),
            timestamp: String::new(),
        });
    }
    let start_time = Instant::now();
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    let mut results = Vec::new();
    for app in &app_names {
        match backup_app(
            &host,
            app,
            &backup_dest,
            &ssh_key_path,
            &timestamp,
            &parameters,
            &playbooks_dir,
        ) {
            Ok(size) => results.push((app.clone(), true, Some(size), None)),
            Err(e) => {
                eprintln!("✗ {} backup failed: {}", app, e);
                results.push((app.clone(), false, None, Some(e.to_string())));
            }
        }
    }

    let elapsed = start_time.elapsed().as_secs();
    let total_size: u64 = results.iter().filter_map(|(_, _, size, _)| *size).sum();
    let successful = results.iter().filter(|(_, ok, _, _)| *ok).count();
    let failed = results.iter().filter(|(_, ok, _, _)| !*ok).count();

    eprintln!();

    if output::is_verbose() {
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

    if output::is_verbose() {
        output::info(&format!(
            "Location: {}/{}/",
            backup_dest.join(&host.name).display(),
            timestamp
        ));
    }

    let outcome = CreateOutcome {
        successful_apps: results
            .iter()
            .filter(|(_, ok, _, _)| *ok)
            .map(|(name, _, _, _)| name.to_string())
            .collect(),
        failed_apps: results
            .into_iter()
            .filter(|(_, ok, _, _)| !*ok)
            .map(|(name, _, _, err)| (name.to_string(), err.unwrap_or_default()))
            .collect(),
        timestamp,
    };

    Ok(outcome)
}

pub fn run_backup_sync(
    host: Option<String>,
    apps: Option<Vec<String>>,
    ssh_key: Option<PathBuf>,
    include_music: bool,
    dry_run: bool,
) -> Result<()> {
    let resolved = get_host_or_select(host)?;
    let host_name = resolved.name.clone();
    output::info(&format!("Starting backup sync pipeline for {}", host_name));

    let outcome = run_backup_create(
        Some(host_name.clone()),
        apps,
        None,
        ssh_key,
        include_music,
        dry_run,
    )?;

    if dry_run {
        output::info("Dry run: would next push to restic, prune, and clean up local staging");
        return Ok(());
    }

    let staging_dir = default_backup_dir()
        .join(&host_name)
        .join(&outcome.timestamp);

    if outcome.successful_apps.is_empty() {
        let _ = fs::remove_dir_all(&staging_dir);
        eyre::bail!(
            "All {} app(s) failed; nothing to push",
            outcome.failed_apps.len()
        );
    }

    if !outcome.failed_apps.is_empty() {
        let names: Vec<&str> = outcome
            .failed_apps
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        output::warn(&format!(
            "Continuing push/prune with {} succeeded, {} failed: {}",
            outcome.successful_apps.len(),
            outcome.failed_apps.len(),
            names.join(", ")
        ));
    }

    run_backup_push(Some(host_name), Some(outcome.timestamp.clone()))?;

    if let Err(e) = run_backup_prune(false) {
        output::warn(&format!("Prune failed (push succeeded): {}", e));
    }

    cleanup_staging_dir(&staging_dir)?;

    if !outcome.failed_apps.is_empty() {
        eyre::bail!(
            "Sync completed with {} app failure(s); push/prune ran on {} successful app(s)",
            outcome.failed_apps.len(),
            outcome.successful_apps.len()
        );
    }

    output::success("Sync complete: create \u{2192} push \u{2192} prune \u{2192} cleanup");

    Ok(())
}

fn cleanup_staging_dir(staging_dir: &Path) -> Result<()> {
    fs::remove_dir_all(staging_dir)
        .wrap_err_with(|| format!("Failed to clean up staging dir: {}", staging_dir.display()))?;
    output::success(&format!(
        "Cleaned up local staging ({})",
        staging_dir.display()
    ));
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
        if !confirm("Continue with restore?", opts.yes) {
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

        let emergency_result = run_backup_create(
            Some(host.name.clone()),
            Some(app_names.clone()),
            Some(backup_root.clone()),
            Some(ssh_key_path.clone()),
            false,
            false,
        )
        .and_then(|outcome| {
            if outcome.failed_apps.is_empty() {
                Ok(())
            } else {
                eyre::bail!("{} backup(s) failed", outcome.failed_apps.len());
            }
        });

        match emergency_result {
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

                if !confirm("Continue without emergency backup?", false) {
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

        let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
        let apps_playbook = assets.playbooks_dir().join("apps.yml");

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

            // Build a Preflight — best-effort; if config is incomplete we warn and skip.
            let preflight_result =
                Config::load().and_then(|cfg| cfg.preflight_for("apps.yml", Some(&tags)));

            match preflight_result {
                Err(e) => {
                    eprintln!(
                        "⚠ Skipping Ansible playbook (config validation failed): {}",
                        e
                    );
                    eprintln!("  Services may fail due to incorrect file ownership!");
                    eprintln!(
                        "  Fix manually: cd ansible && ansible-playbook playbooks/apps.yml --tags {}",
                        tags.join(",")
                    );
                }
                Ok(preflight) => match crate::services::ansible_runner::run_playbook(
                    &preflight,
                    &apps_playbook,
                    &inventory_host,
                    false,
                    Some(&tags),
                    None,
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
                },
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
        let cross_host_dir = assets_playbooks_dir().ok();
        let recipes_for_apps: Vec<(String, Vec<String>)> = match &cross_host_dir {
            Some(dir) => app_names
                .iter()
                .filter_map(|name| {
                    load_app_recipe(dir, name)
                        .ok()
                        .map(|r| (name.clone(), r.systemd_services))
                })
                .collect(),
            None => Vec::new(),
        };
        let all_services: Vec<&str> = recipes_for_apps
            .iter()
            .flat_map(|(_, services)| services.iter().map(String::as_str))
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
        for (_app_name, services) in &recipes_for_apps {
            for service in services {
                eprintln!(
                    "     ssh {}@{} 'journalctl -u {} --since \"5 minutes ago\" | grep -i error'",
                    host.user, host.address, service
                );
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

    let playbooks_dir = assets_playbooks_dir()?;
    let recipe = load_app_recipe(&playbooks_dir, app_name)
        .wrap_err_with(|| format!("Unknown or non-backuppable app: {}", app_name))?;

    let session = LiveSshSession::new(host, ssh_key);
    let executor = RecipeExecutor::new(&session);
    let pb = output::progress_bar(&format!("Restoring {}", app_name), None);
    let result = executor.restore(&recipe, backup_path, &HashMap::new());
    pb.finish_and_clear();
    result?;

    eprintln!("✓ {} restore completed", app_name);
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
    let config = Config::load()?;
    let missing = config.validate_required(&["restic_repository", "restic_password"]);
    if !missing.is_empty() {
        eyre::bail!(
            "Missing restic config: {}. Set with `auberge config set <key> <value>`",
            missing.join(", ")
        );
    }
    Ok((
        config
            .get_resolved("restic_repository")?
            .ok_or_else(|| eyre::eyre!("restic_repository is missing or not a valid value"))?,
        config
            .get_resolved("restic_password")?
            .ok_or_else(|| eyre::eyre!("restic_password is missing or not a valid value"))?,
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
        .env_remove("RESTIC_PASSWORD_COMMAND")
        .output();

    let needs_init = match snapshots_check {
        Ok(out) => {
            let stderr_text = String::from_utf8_lossy(&out.stderr);
            let lines = output::subprocess_output("restic", &stderr_text);
            if out.status.success() {
                output::clear_subprocess_lines(lines);
                false
            } else if stderr_text.contains("Is there a repository at the following location")
                || stderr_text.contains("unable to open config file")
            {
                output::clear_subprocess_lines(lines);
                true
            } else {
                eyre::bail!("restic snapshots failed: {}", stderr_text.trim());
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
            .env_remove("RESTIC_PASSWORD_COMMAND")
            .output()
            .wrap_err("Failed to initialize restic repository")?;
        let stderr_text = String::from_utf8_lossy(&init_output.stderr);
        let lines = output::subprocess_output("restic", &stderr_text);
        if init_output.status.success() {
            output::clear_subprocess_lines(lines);
        }

        if !init_output.status.success() {
            eyre::bail!(
                "Failed to initialize restic repository: {}",
                stderr_text.trim()
            );
        }
    }

    spinner.finish_and_clear();

    let pb = output::progress_bar(&format!("Pushing {}", backup_dir.display()), None);
    let mut snapshot_id: Option<String> = None;

    let result = output::run_with_stdout_progress(
        "restic",
        Command::new("restic")
            .arg("backup")
            .arg("--json")
            .arg(&backup_dir)
            .env("RESTIC_REPOSITORY", &restic_repo)
            .env("RESTIC_PASSWORD", &restic_password)
            .env_remove("RESTIC_PASSWORD_COMMAND"),
        &pb,
        |line, pb| match output::parse_restic_message(line) {
            Some(output::ResticMessage::Status(s)) => {
                if let (Some(total), Some(done)) = (s.total_bytes, s.bytes_done) {
                    if pb.length() != Some(total) {
                        output::set_bytes_style(pb);
                        pb.set_length(total);
                    }
                    pb.set_position(done);
                } else {
                    if pb.length() != Some(100) {
                        output::set_percent_style(pb);
                        pb.set_length(100);
                    }
                    pb.set_position((s.percent_done * 100.0) as u64);
                }
            }
            Some(output::ResticMessage::Summary(s)) => {
                snapshot_id = Some(s.snapshot_id);
            }
            None => {}
        },
    )
    .wrap_err("Failed to run restic backup")?;

    pb.finish_and_clear();

    if !result.status.success() {
        if result.last_stderr.is_empty() {
            eyre::bail!("restic backup failed");
        } else {
            eyre::bail!("restic backup failed: {}", result.last_stderr.trim());
        }
    }

    match snapshot_id {
        Some(id) => output::success(&format!("Push complete: snapshot {}", id)),
        None => output::success("Push complete"),
    };

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
        .env("RESTIC_PASSWORD", &restic_password)
        .env_remove("RESTIC_PASSWORD_COMMAND");

    if dry_run {
        cmd.arg("--dry-run");
    }

    let prune_output = cmd.output().wrap_err("Failed to run restic forget")?;

    spinner.finish_and_clear();

    let stderr_text = String::from_utf8_lossy(&prune_output.stderr);
    let lines = output::subprocess_output("restic", &stderr_text);
    if prune_output.status.success() {
        output::clear_subprocess_lines(lines);
    }

    if !prune_output.status.success() {
        eyre::bail!("restic prune failed: {}", stderr_text.trim());
    }

    let stdout = String::from_utf8_lossy(&prune_output.stdout);
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
                let selection = crate::prompt::select_item(
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
    hosts_select_or_arg(host_arg)
}

fn default_backup_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("auberge").join("backups"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/auberge/backups"))
}

fn backup_app(
    host: &Host,
    app_name: &str,
    backup_dest: &Path,
    ssh_key: &Path,
    timestamp: &str,
    parameters: &HashMap<String, bool>,
    playbooks_dir: &Path,
) -> Result<u64> {
    let recipe = load_app_recipe(playbooks_dir, app_name)?;
    let pb = output::progress_bar(&format!("Backing up {}", app_name), None);
    let app_backup_dir = backup_dest.join(&host.name).join(timestamp).join(app_name);

    fs::create_dir_all(&app_backup_dir).wrap_err_with(|| {
        format!(
            "Failed to create backup directory: {}",
            app_backup_dir.display()
        )
    })?;

    let session = LiveSshSession::new(host, ssh_key);
    let executor = RecipeExecutor::new(&session);
    let exec_result = executor.backup(&recipe, &app_backup_dir, parameters);

    pb.finish_and_clear();

    if let Err(e) = exec_result {
        let _ = fs::remove_dir_all(&app_backup_dir);
        return Err(e);
    }

    let backup_size = calculate_dir_size(&app_backup_dir)?;
    if !output::is_verbose() {
        output::success(&format!(
            "{} ({})",
            app_name,
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
        Ok(out) if out.status.success() => {
            let lines = output::subprocess_output("ssh", &String::from_utf8_lossy(&out.stderr));
            output::clear_subprocess_lines(lines);
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
    let playbooks_dir = assets_playbooks_dir().ok();
    for app in apps {
        let recipe = match playbooks_dir
            .as_ref()
            .and_then(|d| load_app_recipe(d, app).ok())
        {
            Some(r) => r,
            None => continue,
        };
        for service in &recipe.systemd_services {
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
    fn test_parameter_map_carries_include_music_flag() {
        let p = parameter_map(true);
        assert_eq!(p.get("include_music").copied(), Some(true));
        let p = parameter_map(false);
        assert_eq!(p.get("include_music").copied(), Some(false));
    }

    #[test]
    fn create_outcome_records_partial_success() {
        let outcome = CreateOutcome {
            successful_apps: vec!["paperless".to_string(), "freshrss".to_string()],
            failed_apps: vec![("bichon".to_string(), "Unit not loaded".to_string())],
            timestamp: "2026-04-28_03-00-00".to_string(),
        };
        assert_eq!(outcome.successful_apps.len(), 2);
        assert_eq!(outcome.failed_apps.len(), 1);
        assert_eq!(outcome.failed_apps[0].0, "bichon");
        assert_eq!(outcome.timestamp, "2026-04-28_03-00-00");
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

    fn test_host() -> Host {
        Host {
            name: "test".to_string(),
            address: "192.0.2.1".to_string(),
            user: "deploy".to_string(),
            port: 2222,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
        }
    }

    #[test]
    fn test_ssh_args_contains_mux_options() {
        let host = test_host();
        let key = Path::new("/home/user/.ssh/id_ed25519");
        let session = SshSession::new(&host, key);
        let args = session.ssh_args();
        let strs: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(strs.contains(&"ControlMaster=auto".to_string()));
        assert!(strs.contains(&"ControlPath=/tmp/ssh-%r@%h:%p".to_string()));
        assert!(strs.contains(&"ControlPersist=60s".to_string()));
    }

    #[test]
    fn test_ssh_args_includes_key_port_user_host() {
        let host = test_host();
        let key = Path::new("/home/user/.ssh/id_ed25519");
        let session = SshSession::new(&host, key);
        let args = session.ssh_args();
        let strs: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(strs.contains(&"/home/user/.ssh/id_ed25519".to_string()));
        assert!(strs.contains(&"2222".to_string()));
        assert!(strs.contains(&"deploy@192.0.2.1".to_string()));
    }

    #[test]
    fn test_scp_args_uses_uppercase_p_for_port() {
        let host = test_host();
        let key = Path::new("/tmp/key");
        let session = SshSession::new(&host, key);
        let args = session.scp_args();
        let strs: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(strs.contains(&"-P".to_string()));
        assert!(!strs.contains(&"-p".to_string()));
    }

    #[test]
    fn test_rsync_e_arg_contains_mux_and_key() {
        let host = test_host();
        let key = Path::new("/home/user/.ssh/id_ed25519");
        let session = SshSession::new(&host, key);
        let e_arg = session.rsync_e_arg();
        assert!(e_arg.starts_with("ssh "));
        assert!(e_arg.contains("ControlMaster=auto"));
        assert!(e_arg.contains("ControlPath=/tmp/ssh-%r@%h:%p"));
        assert!(e_arg.contains("ControlPersist=60s"));
        assert!(e_arg.contains("-i /home/user/.ssh/id_ed25519"));
        assert!(e_arg.contains("-p 2222"));
    }

    #[test]
    fn test_rsync_e_arg_escapes_spaces_in_key_path() {
        let host = test_host();
        let key = Path::new("/home/user/my keys/id_ed25519");
        let session = SshSession::new(&host, key);
        let e_arg = session.rsync_e_arg();
        assert!(!e_arg.contains("-i /home/user/my keys/id_ed25519"));
        assert!(e_arg.contains("'/home/user/my keys/id_ed25519'"));
    }

    #[test]
    fn test_sync_variant_exists() {
        let _sync = BackupCommands::Sync {
            host: Some("myserver".to_string()),
            apps: None,
            ssh_key: None,
            include_music: false,
            dry_run: true,
        };
    }

    #[test]
    fn test_cleanup_staging_dir_removes_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("2026-04-06_03-00-00");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("data.bin"), vec![0u8; 1024]).unwrap();

        assert!(staging.exists());
        cleanup_staging_dir(&staging).unwrap();
        assert!(!staging.exists());
    }

    #[test]
    fn test_cleanup_staging_dir_fails_on_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("nonexistent");
        assert!(cleanup_staging_dir(&staging).is_err());
    }

    #[test]
    fn test_resolve_backup_dir_selects_newest_for_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir_all(host_dir.join("2026-04-05_03-00-00")).unwrap();
        fs::create_dir_all(host_dir.join("2026-04-06_03-00-00")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            host_dir.join("2026-04-06_03-00-00"),
            host_dir.join("latest"),
        )
        .unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("myserver"), None).unwrap();
        assert_eq!(result, host_dir.join("2026-04-06_03-00-00"));
    }

    #[test]
    fn test_mux_args_pairs_options_correctly() {
        let args = SshSession::mux_args();
        let strs: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        for (i, s) in strs.iter().enumerate() {
            if s == "-o" {
                assert!(
                    strs[i + 1].contains('='),
                    "option after -o should be key=value"
                );
            }
        }
    }
}
