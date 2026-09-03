use crate::commands::opml::OpmlCommands;
use crate::config::Config;
use crate::hosts::{HOST_FLAG, Host, HostManager, select_or_arg as hosts_select_or_arg};
use crate::output;
use crate::playbook_meta::unit_file_name;
use crate::prompt::confirm;
use crate::services::ansible_runner::AnsibleResult;
use crate::services::backup::executor::staged_paths;
use crate::services::backup::recipe::{
    assets_playbooks_dir, discover_backuppable_apps, load_app_recipe,
};
use crate::services::backup::restic;
use crate::services::backup::restore::{
    EmergencyOutcome, RedeployOutcome, RestoreOutcome, RestorePhase, RestoreSession,
    RestoreSessionOpts, RestoreTarget,
};
use crate::services::backup::session::{
    BackupSession, CreateOutcome, SessionOpts, calculate_dir_size, restic_prune, restic_push,
};
use crate::services::backup::verify::{self, MaxAge, Status, Verdict, VerifyRequest};
use crate::services::progress::{Progress, TerminalProgress};
use crate::services::ssh::{CONNECT_TIMEOUT, LiveSshSession, SshSession, resolve_ssh_key_path};
use chrono::Utc;
use clap::Subcommand;
use eyre::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use tabled::Tabled;

fn backup_timestamp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}$")
            .expect("backup timestamp regex must compile")
    })
}

fn is_backup_timestamp_dir(entry: &fs::DirEntry) -> bool {
    let path = entry.path();
    if !path.is_dir() || path.is_symlink() {
        return false;
    }
    let name = entry.file_name();
    let name = name.to_string_lossy();
    backup_timestamp_re().is_match(&name)
}

#[derive(Subcommand)]
pub enum BackupCommands {
    #[command(visible_alias = "c", about = "Create backup of application data")]
    Create {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to backup (actual,baikal,bichon,freshrss,gokapi,headscale,navidrome,calibre,yourls,paperless). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(short, long, help = "Backup destination directory")]
        dest: Option<PathBuf>,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{host}/{user})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(long, help = "Include music files in Navidrome backup (large, slow)")]
        include_music: bool,
        #[arg(short = 'n', long, help = "Dry run (show what would be backed up)")]
        dry_run: bool,
    },
    #[command(
        visible_alias = "s",
        about = "Create backup, push to restic, prune, and clean up local staging"
    )]
    Sync {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Apps to backup (actual,baikal,bichon,freshrss,gokapi,headscale,navidrome,calibre,yourls,paperless). Default: all"
        )]
        apps: Option<Vec<String>>,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{host}/{user})"
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
    #[command(visible_alias = "ls", about = "List available backups")]
    List {
        #[arg(short = 'H', long, help = "Filter by host")]
        host: Option<String>,
        #[arg(short, long, help = "Filter by app")]
        app: Option<String>,
        #[command(flatten)]
        output: OutputArg,
    },
    #[command(visible_alias = "r", about = "Restore from backup")]
    Restore {
        #[arg(help = "Backup timestamp (YYYY-MM-DD_HH-MM-SS) or 'latest' (omit to be prompted)")]
        backup_id: Option<String>,
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
            help = "Apps to restore (default: pick from the apps present in the backup)"
        )]
        apps: Option<Vec<String>>,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{host}/{user})"
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
    #[command(
        visible_alias = "p",
        about = "Push backups to offsite restic repository"
    )]
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
    #[command(
        visible_alias = "v",
        about = "Check the latest offsite snapshot is fresh and holds an app's backup"
    )]
    Verify {
        #[arg(
            short = 'H',
            long,
            help = "Host whose snapshots to check (default: the sole configured host)"
        )]
        host: Option<String>,
        #[arg(short, long, help = "Also assert this app is in the latest snapshot")]
        app: Option<String>,
        #[arg(
            long,
            default_value = "24h",
            help = "Freshness threshold as <number><s|m|h|d>"
        )]
        max_age: String,
        #[command(flatten)]
        output: OutputArg,
    },
    /// Data portability, not backup: freshrss's Backup Recipe already carries
    /// the app's data directories. Flattened rather than moved so the surface
    /// users type — `backup export-opml`, `eo`, `io` — is unchanged (#673).
    #[command(flatten)]
    Opml(OpmlCommands),
}

use crate::output::OutputArg;
pub use crate::output::OutputFormat;

pub struct RestoreOptions {
    pub backup_id: Option<String>,
    pub host_arg: Option<String>,
    pub from_host_arg: Option<String>,
    pub apps: Option<Vec<String>>,
    pub ssh_key: Option<PathBuf>,
    pub dry_run: bool,
    pub yes: bool,
    pub skip_playbook_unsafe: bool,
}

/// The parameter map for a `backup create` driven by CLI flags.
pub fn create_parameters(include_music: bool) -> HashMap<String, bool> {
    HashMap::from([("include_music".to_string(), include_music)])
}

pub fn run_backup_create(
    host_arg: Option<String>,
    apps: Option<Vec<String>>,
    dest: Option<PathBuf>,
    ssh_key: Option<PathBuf>,
    parameters: HashMap<String, bool>,
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
            .filter(|name| load_app_recipe(&playbooks_dir, name, &host.user).is_ok())
            .collect(),
        None => discover_backuppable_apps(&playbooks_dir)?,
    };

    if app_names.is_empty() {
        eyre::bail!("No valid apps specified for backup");
    }

    if output::is_verbose() {
        output::info(&format!("Apps: {}", app_names.join(", ")));
    }

    if dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(CreateOutcome {
            results: Vec::new(),
            timestamp: String::new(),
        });
    }

    let recipes: Vec<(String, _)> = app_names
        .iter()
        .map(|name| {
            Ok::<_, eyre::Report>((
                name.clone(),
                load_app_recipe(&playbooks_dir, name, &host.user)?,
            ))
        })
        .collect::<Result<_>>()?;

    let start_time = Instant::now();
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    let opts = SessionOpts {
        host_name: host.name.clone(),
        dest: backup_dest.clone(),
        timestamp: timestamp.clone(),
        parameters,
    };
    let route = crate::services::route::resolve(&host, Some(ssh_key_path))?;
    let ssh = LiveSshSession::new(&route, &host.become_method)?;
    // `--verbose` renders every App's outcome in the results table below, so
    // the streamed per-App line would report each one twice.
    let stream_results = !output::is_verbose();
    let session = BackupSession::new(&ssh, recipes, opts, move |app| {
        let bar = TerminalProgress::new(&format!("Backing up {}", app));
        if stream_results {
            Box::new(bar)
        } else {
            Box::new(ResultsSuppressed(Box::new(bar))) as Box<dyn Progress>
        }
    });
    let outcome = session.create()?;

    render_create_outcome(&outcome, &backup_dest, &host.name, start_time);

    Ok(outcome)
}

/// A `Progress` that drops `success`, forwarding every other event.
///
/// Whether an App's result is streamed or tabled is the command's render
/// policy, not the Backup Session's: the Session emits one `success` per App
/// and does not know what renders it (ADR-0047).
struct ResultsSuppressed(Box<dyn Progress>);

impl Progress for ResultsSuppressed {
    fn task_started(&mut self, name: &str) {
        self.0.task_started(name);
    }

    fn task_done(&mut self) {
        self.0.task_done();
    }

    fn bytes_transferred(&mut self, n: u64) {
        self.0.bytes_transferred(n);
    }

    fn set_total(&mut self, n: Option<u64>) {
        self.0.set_total(n);
    }

    fn info(&mut self, msg: &str) {
        self.0.info(msg);
    }

    fn warn(&mut self, msg: &str) {
        self.0.warn(msg);
    }

    fn success(&mut self, _msg: &str) {}

    fn error(&mut self, msg: &str) {
        self.0.error(msg);
    }

    fn line(&mut self, text: &str) {
        self.0.line(text);
    }
}

fn render_create_outcome(
    outcome: &CreateOutcome,
    backup_dest: &Path,
    host_name: &str,
    start_time: Instant,
) {
    let elapsed = start_time.elapsed().as_secs();
    let successful = outcome.successful_apps().len();
    let failed = outcome.failed_apps().len();
    let total_size = outcome.total_size();

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

        let table_data: Vec<BackupResult> = outcome
            .results
            .iter()
            .map(|r| BackupResult {
                app: r.app.clone(),
                status: match &r.error {
                    None => "✓".to_string(),
                    Some(err) => format!("✗ {}", err),
                },
                size: r.size_bytes.map(output::format_size).unwrap_or_default(),
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
            backup_dest.join(host_name).display(),
            outcome.timestamp
        ));
    }
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
        create_parameters(include_music),
        dry_run,
    )?;

    if dry_run {
        output::info("Dry run: would next push to restic, prune, and clean up local staging");
        return Ok(());
    }

    let staging_dir = default_backup_dir()
        .join(&host_name)
        .join(&outcome.timestamp);

    let successful = outcome.successful_apps();
    let failed = outcome.failed_apps();

    if successful.is_empty() {
        let _ = fs::remove_dir_all(&staging_dir);
        eyre::bail!("All {} app(s) failed; nothing to push", failed.len());
    }

    if !failed.is_empty() {
        let names: Vec<&str> = failed.iter().map(|(name, _)| name.as_str()).collect();
        output::warn(&format!(
            "Continuing push/prune with {} succeeded, {} failed: {}",
            successful.len(),
            failed.len(),
            names.join(", ")
        ));
    }

    run_backup_push(Some(host_name), Some(outcome.timestamp.clone()))?;

    if let Err(e) = run_backup_prune(false) {
        output::warn(&format!("Prune failed (push succeeded): {}", e));
    }

    cleanup_staging_dir(&staging_dir)?;

    if !failed.is_empty() {
        eyre::bail!(
            "Sync completed with {} app failure(s); push/prune ran on {} successful app(s)",
            failed.len(),
            successful.len()
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
        OutputFormat::Human => print_backups_table(&backups),
        OutputFormat::Json => print_backups_json(&backups)?,
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

fn print_backups_table(backups: &[BackupEntry]) {
    let display_backups: Vec<BackupDisplay> = backups.iter().map(BackupDisplay::from).collect();
    output::print_table(&display_backups);
    eprintln!("\nTotal: {} backup(s)", backups.len());
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

pub fn run_backup_restore(opts: RestoreOptions) -> Result<()> {
    let host = get_host_or_select(opts.host_arg)?;
    let backup_root = default_backup_dir();

    let (source_host_name, is_cross_host) = match opts.from_host_arg {
        Some(ref from_host) => (from_host.clone(), from_host != &host.name),
        None => (host.name.clone(), false),
    };

    let host_backup_dir = backup_root.join(&source_host_name);

    if !host_backup_dir.exists() {
        eyre::bail!("No backups found for host: {}", source_host_name);
    }

    let backup_id = match opts.backup_id {
        Some(id) => id,
        None => select_backup_id(&host_backup_dir)?,
    };

    let timestamp_dir = resolve_timestamp_dir(&host_backup_dir, &backup_id)?;

    let app_names = match opts.apps {
        Some(apps) => apps,
        None => select_restore_apps(&timestamp_dir)?,
    };

    let ssh_key_path = resolve_ssh_key_path(&host, opts.ssh_key)?;
    eprintln!("Using SSH key: {}", ssh_key_path.display());

    let playbooks_dir = assets_playbooks_dir()?;
    let mut restore_plan = Vec::new();

    for app_name in &app_names {
        let backup_path = timestamp_dir.join(app_name);
        if !backup_path.exists() {
            eprintln!(
                "⚠ No backup found for {} in {}, skipping",
                app_name,
                timestamp_dir.display()
            );
            continue;
        }

        let recipe = load_app_recipe(&playbooks_dir, app_name, &host.user)
            .wrap_err_with(|| format!("Unknown or non-backuppable app: {}", app_name))?;
        restore_plan.push(RestoreTarget {
            app: app_name.clone(),
            backup_path,
            recipe,
        });
    }

    if restore_plan.is_empty() {
        eyre::bail!("No backups to restore");
    }

    // One session for pre-flight and restore, so the socket the reachability
    // probe warms is the one every later command reuses.
    let route = crate::services::route::resolve(&host, Some(ssh_key_path))?;
    let ssh = LiveSshSession::new(&route, &host.become_method)?;

    if is_cross_host {
        let total_backup_size: u64 = restore_plan
            .iter()
            .map(|target| calculate_dir_size(&target.backup_path).unwrap_or(0))
            .sum();
        validate_cross_host_restore(&ssh, &host, &app_names, total_backup_size)?;
    }

    eprintln!("\n=== Restore Plan ===");
    if is_cross_host {
        eprintln!("Source: {} (backup: {})", source_host_name, backup_id);
        eprintln!("Target: {} ({}:{})", host.name, host.address, host.port);
        eprintln!("\n⚠  CROSS-HOST RESTORE WARNING");
        eprintln!(
            "   This will restore data from '{}' to '{}'",
            source_host_name, host.name
        );
        eprintln!("   Existing data on '{}' will be OVERWRITTEN", host.name);
    } else {
        eprintln!("Host: {}", host.name);
        eprintln!("Backup ID: {}", backup_id);
    }
    eprintln!("\nApps to restore:");
    for target in &restore_plan {
        eprintln!(
            "  - {:<12} from {}",
            target.app,
            target.backup_path.display()
        );
        for path in staged_paths(&target.recipe, &target.backup_path) {
            eprintln!("      → {}", path);
        }
    }

    if opts.dry_run {
        eprintln!("\n✓ Dry run completed (no changes made)");
        return Ok(());
    }

    if is_cross_host && !opts.yes {
        eprintln!("\n⚠  DANGER: Cross-host restore requires explicit confirmation");
        eprintln!("   Type the target host name '{}' to confirm:", host.name);

        if !crate::prompt::confirm_typed("Target host name", &host.name, false)? {
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

    if is_cross_host {
        eprintln!("\n--- Creating Emergency Backup ---");
        eprintln!(
            "  Backing up current state of '{}' before cross-host restore",
            host.name
        );
    }

    let session = RestoreSession::new(
        &ssh,
        &restore_plan,
        RestoreSessionOpts {
            host_name: host.name.clone(),
            backup_root: backup_root.clone(),
            emergency_timestamp: Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string(),
            cross_host: is_cross_host,
        },
        |phase, app| match phase {
            RestorePhase::EmergencyBackup => {
                Box::new(TerminalProgress::new(&format!("Backing up {}", app)))
            }
            RestorePhase::AppRestore => {
                eprintln!("\n--- Restoring {} ---", app);
                Box::new(TerminalProgress::new(&format!("Restoring {}", app)))
            }
        },
        || {
            eprintln!("\n✓ All restores completed successfully");
            if opts.skip_playbook_unsafe {
                RedeployOutcome::SkippedUnsafe
            } else {
                ansible_redeploy(&host, &app_names)
            }
        },
        |error| {
            eprintln!("  ⚠ Failed to create emergency backup: {}", error);
            eprintln!("    Continue without emergency backup? This is DANGEROUS!");
            confirm("Continue without emergency backup?", false)
        },
    );

    let outcome = session.restore();
    render_restore_outcome(
        &outcome,
        &restore_plan,
        &host,
        &backup_root,
        &app_names,
        is_cross_host,
    )
}

/// Re-run the apps playbook over the restored apps so ownership and
/// permissions match what the roles declare. The Restore Session's redeploy
/// capability: command-layer code, so it renders its own run and hands the
/// Session only the outcome.
fn ansible_redeploy(host: &Host, apps: &[String]) -> RedeployOutcome {
    eprintln!("\nRunning Ansible playbooks to fix permissions...");
    match run_apps_playbook(host, apps) {
        Ok(result) if result.success => {
            eprintln!("✓ Ansible playbooks completed successfully");
            eprintln!("  File permissions have been corrected");
            RedeployOutcome::Completed
        }
        Ok(result) => failed_redeploy(apps, format!("exit code: {}", result.exit_code)),
        Err(e) => failed_redeploy(apps, format!("{e:#}")),
    }
}

fn failed_redeploy(apps: &[String], reason: String) -> RedeployOutcome {
    eprintln!("⚠ Ansible playbook failed: {}", reason);
    eprintln!("  Services may fail due to incorrect file ownership!");
    eprintln!(
        "  Fix manually: cd ansible && ansible-playbook playbooks/apps.yml --tags {}",
        apps.join(",")
    );
    RedeployOutcome::Failed(reason)
}

fn run_apps_playbook(host: &Host, apps: &[String]) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    let apps_playbook = assets.playbooks_dir().join("apps.yml");
    if !apps_playbook.exists() {
        eyre::bail!("Ansible playbook not found: {}", apps_playbook.display());
    }

    let preflight = Config::load()
        .and_then(|cfg| {
            crate::services::required_keys::preflight_for(
                &cfg,
                assets.ansible_dir(),
                "apps.yml",
                Some(apps),
                &host.name,
            )
        })
        .wrap_err("config validation failed")?;

    let inventory_host = crate::services::ansible_runner::InventoryHost {
        name: host.name.clone(),
        route: crate::services::route::resolve(host, None)?,
        groups: host.tags.clone(),
    };

    let app_versions = crate::playbook_meta::app_version_vars(&assets.playbooks_dir())?;
    let memory_budgets = crate::playbook_meta::app_memory_vars(&assets.playbooks_dir())?;
    let extra_vars: Vec<(&str, &str)> = app_versions
        .iter()
        .chain(memory_budgets.iter())
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();

    let mut progress = TerminalProgress::new("");
    crate::services::ansible_runner::run_playbook(
        &preflight,
        &apps_playbook,
        &inventory_host,
        false,
        Some(apps),
        None,
        Some(&extra_vars),
        false,
        false,
        false,
        &mut progress,
    )
}

fn render_restore_outcome(
    outcome: &RestoreOutcome,
    plan: &[RestoreTarget],
    host: &Host,
    backup_root: &Path,
    apps: &[String],
    is_cross_host: bool,
) -> Result<()> {
    let (emergency, redeploy) = match outcome {
        RestoreOutcome::Cancelled { .. } => {
            eprintln!("Restore cancelled");
            return Ok(());
        }
        RestoreOutcome::Failed {
            emergency,
            app,
            error,
        } => {
            render_emergency_backup(emergency, host, backup_root);
            eyre::bail!("Failed to restore {}: {}", app, error);
        }
        RestoreOutcome::Restored {
            emergency,
            redeploy,
        } => (emergency, redeploy),
    };

    if let RedeployOutcome::SkippedUnsafe = redeploy {
        eprintln!("\n⚠️  WARNING: Skipped Ansible playbooks (--skip-playbook-unsafe)");
        eprintln!("⚠️  Services WILL fail until you run:");
        eprintln!(
            "     cd ansible && ansible-playbook playbooks/apps.yml --tags {}",
            apps.join(",")
        );
    }

    render_emergency_backup(emergency, host, backup_root);

    if is_cross_host {
        render_post_restore_actions(plan, host);
    }

    Ok(())
}

/// The rollback pointer. Rendered for failed restores too: a half-overwritten
/// Host is exactly when the operator reaches for it.
fn render_emergency_backup(emergency: &EmergencyOutcome, host: &Host, backup_root: &Path) {
    if let EmergencyOutcome::Created { timestamp } = emergency {
        eprintln!("\n✓ Emergency backup created: pre-migration-{}", timestamp);
        eprintln!(
            "  Location: {}/{}/{}/",
            backup_root.display(),
            host.name,
            timestamp
        );
    }
}

fn render_post_restore_actions(plan: &[RestoreTarget], host: &Host) {
    eprintln!("\n=== Post-Restore Actions Required ===");
    eprintln!("  Cross-host restore completed. Manual verification needed:\n");
    let all_services: Vec<&str> = plan
        .iter()
        .flat_map(|target| target.recipe.systemd_services.iter().map(String::as_str))
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
    for target in plan {
        for service in &target.recipe.systemd_services {
            eprintln!(
                "     ssh {}@{} 'journalctl -u {} --since \"5 minutes ago\" | grep -i error'",
                host.user, host.address, service
            );
        }
    }
    eprintln!("\n  3. Update DNS records if hostnames changed");
    eprintln!("\n  4. Verify SSL certificates are valid for new domain\n");

    let advice = declared_restore_advice(plan);
    if !advice.is_empty() {
        eprintln!("  ⚠  App-specific notes:");
        for (app, note) in &advice {
            eprintln!("     - {app}: {note}");
        }
    }
}

/// Each restored App paired with the `restore_advice` its Recipe declares, in
/// the order the plan restores them. Reads the plan rather than the requested
/// app list, so an App whose backup directory was missing and got skipped
/// contributes nothing. Selection only — the caller owns the formatting.
fn declared_restore_advice(plan: &[RestoreTarget]) -> Vec<(&str, &str)> {
    plan.iter()
        .filter_map(|target| {
            target
                .recipe
                .restore_advice
                .as_deref()
                .map(|advice| (target.app.as_str(), advice))
        })
        .collect()
}

fn load_restic_config() -> Result<(String, String)> {
    let config = Config::load()?;
    let missing = config.validate_required(&["restic_repository", "restic_password"], None);
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

    let host = backup_dir
        .parent()
        .and_then(Path::file_name)
        .map(|h| h.to_string_lossy().into_owned())
        .ok_or_else(|| {
            eyre::eyre!(
                "Cannot determine host from backup dir: {}",
                backup_dir.display()
            )
        })?;

    let mut progress = TerminalProgress::new("");
    restic_push(
        &restic_repo,
        &restic_password,
        &backup_dir,
        &host,
        &mut progress,
    )
}

pub fn run_backup_prune(dry_run: bool) -> Result<()> {
    let (restic_repo, restic_password) = load_restic_config()?;
    let mut progress = TerminalProgress::new("");
    restic_prune(&restic_repo, &restic_password, dry_run, &mut progress)
}

pub struct VerifyOptions {
    pub host: Option<String>,
    pub app: Option<String>,
    pub max_age: String,
    pub format: OutputFormat,
}

/// Returns the process exit code: 0 verified, 1 a check failed, 2 operational
/// error. Verify is a gate for destructive downstream work, so the caller must
/// be able to branch on which of the three happened.
pub fn run_backup_verify(opts: VerifyOptions) -> i32 {
    verify_exit_code(verify_and_report(opts))
}

fn verify_exit_code(result: Result<Status>) -> i32 {
    match result {
        Ok(status) => status.exit_code(),
        Err(e) => {
            eprintln!("✗ {e:#}");
            Status::OperationalError.exit_code()
        }
    }
}

fn verify_and_report(opts: VerifyOptions) -> Result<Status> {
    let max_age = MaxAge::parse(&opts.max_age)?;
    let (restic_repo, restic_password) = load_restic_config()?;
    let host = resolve_snapshot_host(opts.host)?;

    let request = VerifyRequest {
        host: &host,
        app: opts.app.as_deref(),
        max_age: &max_age,
        now: Utc::now(),
    };

    let verdict = match restic::snapshots_json(&restic_repo, &restic_password) {
        Ok(snapshots_json) => verify::verdict(&request, &snapshots_json, |snapshot, path| {
            restic::snapshot_contains_path(&restic_repo, &restic_password, &snapshot.id, path)
        }),
        Err(e) => Verdict::unreachable(&format!("{e:#}")),
    };

    match opts.format {
        OutputFormat::Human => print_verify_checklist(&verdict),
        OutputFormat::Json => print_verify_json(&request, &verdict)?,
    }

    for remediation in verdict
        .checks
        .iter()
        .filter_map(|check| check.remediation.as_deref())
    {
        eprintln!("{}", remediation);
    }

    Ok(verdict.status)
}

fn resolve_snapshot_host(host_arg: Option<String>) -> Result<String> {
    match host_arg {
        Some(name) => Ok(name),
        None => sole_configured_host(&HostManager::load_hosts()?),
    }
}

/// Verify is a scripted gate, so it never prompts: a single configured Host is
/// implied, anything else makes `--host` mandatory.
fn sole_configured_host(hosts: &[Host]) -> Result<String> {
    match hosts {
        [only] => Ok(only.name.clone()),
        [] => eyre::bail!(
            "No hosts configured. Pass --host <name> or add one with `auberge host add`"
        ),
        many => {
            let names: Vec<&str> = many.iter().map(|h| h.name.as_str()).collect();
            eyre::bail!(
                "{} hosts configured; pass --host <{}>",
                many.len(),
                names.join("|")
            )
        }
    }
}

fn print_verify_checklist(verdict: &Verdict) {
    for check in &verdict.checks {
        println!("{} {}", if check.passed { "✓" } else { "✗" }, check.message);
    }
    println!(
        "{}",
        match verdict.is_verified() {
            true => "verified",
            false => "not verified",
        }
    );
}

fn print_verify_json(request: &VerifyRequest<'_>, verdict: &Verdict) -> Result<()> {
    let checks: Vec<_> = verdict
        .checks
        .iter()
        .map(|check| {
            serde_json::json!({
                "name": check.name,
                "passed": check.passed,
                "message": check.message,
                "remediation": check.remediation,
            })
        })
        .collect();

    let snapshot = verdict.snapshot.as_ref().map(|snapshot| {
        serde_json::json!({
            "id": snapshot.id,
            "short_id": snapshot.short_id,
            "time": snapshot.time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "age_seconds": snapshot.age_seconds,
        })
    });

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "verified": verdict.is_verified(),
        "status": verdict.status.as_str(),
        "host": request.host,
        "app": request.app,
        "max_age": request.max_age.label(),
        "snapshot": snapshot,
        "checks": checks,
    }))?;

    println!("{}", json);
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
                    crate::prompt::Choice::new("host backup")
                        .with_prompt("Select host backup to push")
                        .resolved_by("-H <host>"),
                )?;
                backup_root.join(&selection)
            }
        }
    };

    match backup_id {
        Some(id) => resolve_timestamp_dir(&host_dir, id),
        None => latest_timestamp_dir(&host_dir),
    }
}

fn sorted_timestamp_entries(host_backup_dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<_> = fs::read_dir(host_backup_dir)?
        .filter_map(Result::ok)
        .filter(is_backup_timestamp_dir)
        .collect();

    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    Ok(entries)
}

fn latest_timestamp_dir(host_backup_dir: &Path) -> Result<PathBuf> {
    sorted_timestamp_entries(host_backup_dir)?
        .first()
        .map(|e| e.path())
        .ok_or_else(|| {
            eyre::eyre!(
                "No backup timestamps found in {}",
                host_backup_dir.display()
            )
        })
}

fn resolve_timestamp_dir(host_backup_dir: &Path, backup_id: &str) -> Result<PathBuf> {
    if backup_id == "latest" {
        return latest_timestamp_dir(host_backup_dir);
    }

    let dir = host_backup_dir.join(backup_id);
    if !dir.exists() {
        eyre::bail!("Backup not found: {}", dir.display());
    }
    Ok(dir)
}

fn list_restorable_apps(timestamp_dir: &Path) -> Result<Vec<String>> {
    let mut apps: Vec<String> = fs::read_dir(timestamp_dir)?
        .filter_map(Result::ok)
        .filter(|e| !e.path().is_symlink() && e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    apps.sort();
    Ok(apps)
}

fn select_restore_apps(timestamp_dir: &Path) -> Result<Vec<String>> {
    let apps = list_restorable_apps(timestamp_dir)?;
    if apps.is_empty() {
        eyre::bail!("No app backups found in {}", timestamp_dir.display());
    }

    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eyre::bail!("Apps are required in non-interactive mode (pass -a <apps>)");
    }

    crate::prompt::select_multi(&apps, "Select apps to restore")
        .ok_or_else(|| eyre::eyre!("No apps selected"))
}

fn select_backup_id(host_backup_dir: &Path) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        eyre::bail!(
            "Backup ID is required in non-interactive mode (pass it explicitly or use 'latest')"
        );
    }

    let entries = sorted_timestamp_entries(host_backup_dir)?;

    if entries.is_empty() {
        eyre::bail!(
            "No backup timestamps found in {}",
            host_backup_dir.display()
        );
    }

    let mut options = vec!["latest".to_string()];
    options.extend(
        entries
            .iter()
            .map(|e| e.file_name().to_string_lossy().into_owned()),
    );

    crate::prompt::select_item(
        &options,
        |s: &String| s.clone(),
        crate::prompt::Choice::new("backup").resolved_by("the backup ID or 'latest'"),
    )
}

fn get_host_or_select(host_arg: Option<String>) -> Result<Host> {
    hosts_select_or_arg(host_arg, HOST_FLAG)
}

fn default_backup_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("auberge").join("backups"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/auberge/backups"))
}

/// The answer is the unit file name appearing in systemctl's own output, and
/// nothing else: `list-unit-files` prints a header and "0 unit files listed."
/// for a name it does not know, so a non-empty stdout proves nothing, and its
/// exit code for that case is systemd-version-dependent.
///
/// Which leaves the exit code unable to answer the question at all. And-ing it
/// in read an unreachable Host as an absent unit, so the pre-flight aborted
/// the restore telling the operator to re-run a deploy that was never the
/// problem (#693); it also let a systemd that answered under a non-zero exit
/// hide a unit that is installed. [`SshSession::run_raw`] now raises the
/// transport failure itself, and the `?` carries it — leaving this to say only
/// what it can: whether the Host named the unit file.
fn check_remote_unit_exists(session: &dyn SshSession, unit: &str) -> Result<bool> {
    let unit_file = unit_file_name(unit);
    let output = session
        .run_raw(&["systemctl", "list-unit-files", &unit_file])
        .wrap_err_with(|| format!("cannot tell whether {} is installed", unit_file))?;
    Ok(output.stdout_str().contains(&unit_file))
}

fn check_remote_disk_space(session: &dyn SshSession, path: &str) -> Result<u64> {
    let output = session
        .run(&format!("df --output=avail {} | tail -1", path))
        .wrap_err("Failed to check disk space")?;

    if !output.success {
        eyre::bail!("Failed to check disk space on remote host");
    }

    let kb_available = output
        .stdout_str()
        .trim()
        .parse::<u64>()
        .wrap_err("Failed to parse disk space output")?;

    Ok(kb_available * 1024)
}

fn validate_cross_host_restore(
    session: &dyn SshSession,
    host: &Host,
    apps: &[String],
    backup_size_bytes: u64,
) -> Result<()> {
    eprintln!("\n--- Pre-flight Validation ---");

    eprintln!("  Checking SSH connectivity...");
    session.reachable(CONNECT_TIMEOUT)?;
    eprintln!("    ✓ SSH connection successful");

    eprintln!("  Checking services on target...");
    let playbooks_dir = assets_playbooks_dir().ok();
    for app in apps {
        let recipe = match playbooks_dir
            .as_ref()
            .and_then(|d| load_app_recipe(d, app, &host.user).ok())
        {
            Some(r) => r,
            None => continue,
        };
        for service in &recipe.systemd_services {
            // `?`, not the warning this used to print: an unanswered probe
            // has not cleared the unit, and this check is the one the restore
            // treats as a gate.
            if check_remote_unit_exists(session, service)? {
                eprintln!("    ✓ {} service exists", service);
            } else {
                eprintln!("    ⚠ {} service not found on target", service);
                eprintln!(
                    "      Run 'auberge ansible run --host {}' to install services",
                    host.name
                );
                eyre::bail!("Required service {} not found on target host", service);
            }
        }
    }

    eprintln!("  Checking disk space...");
    match check_remote_disk_space(session, "/") {
        Ok(available_bytes) => {
            let required_bytes = (backup_size_bytes as f64 * 1.2) as u64;
            eprintln!(
                "    Available: {}, Required: {} (with 20% buffer)",
                output::format_size(available_bytes),
                output::format_size(required_bytes)
            );

            if available_bytes < required_bytes {
                eyre::bail!(
                    "Insufficient disk space: need {}, have {}",
                    output::format_size(required_bytes),
                    output::format_size(available_bytes)
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
    use crate::playbook_meta::BackupRecipe;
    use crate::services::progress::{MockProgress, ProgressEvent};

    #[test]
    fn results_suppressed_drops_success_and_forwards_the_rest() {
        let recorder = MockProgress::new();
        let mut progress = ResultsSuppressed(Box::new(recorder.share()));

        progress.task_started("Stopping bichon");
        progress.set_total(Some(1024));
        progress.bytes_transferred(512);
        progress.info("informational");
        progress.warn("warning");
        progress.line("verbatim");
        progress.success("bichon (1.00 KB)");
        progress.error("bichon backup failed: no route to host");
        progress.task_done();

        assert_eq!(
            recorder.events(),
            [
                ProgressEvent::TaskStarted("Stopping bichon".to_string()),
                ProgressEvent::SetTotal(Some(1024)),
                ProgressEvent::BytesTransferred(512),
                ProgressEvent::Info("informational".to_string()),
                ProgressEvent::Warn("warning".to_string()),
                ProgressEvent::Line("verbatim".to_string()),
                ProgressEvent::Error("bichon backup failed: no route to host".to_string()),
                ProgressEvent::TaskDone,
            ]
        );
    }

    fn recipe_advising(advice: Option<&str>) -> BackupRecipe {
        BackupRecipe {
            systemd_services: Vec::new(),
            paths: Vec::new(),
            attests: None,
            owner: None,
            db: None,
            post_restore_command: None,
            restore_advice: advice.map(str::to_string),
            parameters: HashMap::new(),
        }
    }

    fn target(app: &str, advice: Option<&str>) -> RestoreTarget {
        RestoreTarget {
            app: app.to_string(),
            backup_path: PathBuf::from("/tmp").join(app),
            recipe: recipe_advising(advice),
        }
    }

    #[test]
    fn declared_restore_advice_pairs_each_declaring_app_in_plan_order() {
        let plan = vec![
            target("navidrome", Some("rescan the library")),
            target("freshrss", Some("verify feeds update")),
        ];

        assert_eq!(
            declared_restore_advice(&plan),
            vec![
                ("navidrome", "rescan the library"),
                ("freshrss", "verify feeds update"),
            ]
        );
    }

    #[test]
    fn declared_restore_advice_skips_a_recipe_that_declares_none() {
        let plan = vec![
            target("baikal", None),
            target("navidrome", Some("rescan the library")),
        ];

        assert_eq!(
            declared_restore_advice(&plan),
            vec![("navidrome", "rescan the library")]
        );
    }

    #[test]
    fn declared_restore_advice_is_empty_when_no_app_declares_any() {
        let plan = vec![target("baikal", None), target("gokapi", None)];

        assert!(
            declared_restore_advice(&plan).is_empty(),
            "a plan with no declared advice must render no section at all"
        );
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
    fn test_verify_variant_exists() {
        let _verify = BackupCommands::Verify {
            host: None,
            app: Some("bichon".to_string()),
            max_age: "24h".to_string(),
            output: OutputArg {
                format: OutputFormat::Human,
            },
        };
    }

    #[test]
    fn sole_configured_host_is_implied() {
        let hosts = vec![test_host()];
        assert_eq!(sole_configured_host(&hosts).unwrap(), "test");
    }

    #[test]
    fn sole_configured_host_errors_without_hosts() {
        let err = sole_configured_host(&[]).unwrap_err().to_string();
        assert!(err.contains("No hosts configured"), "{err}");
    }

    #[test]
    fn sole_configured_host_errors_with_several_hosts() {
        let second = Host {
            name: "other".to_string(),
            ..test_host()
        };
        let err = sole_configured_host(&[test_host(), second])
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 hosts configured"), "{err}");
        assert!(err.contains("--host <test|other>"), "{err}");
    }

    #[test]
    fn verify_exit_code_maps_each_status() {
        assert_eq!(verify_exit_code(Ok(Status::Verified)), 0);
        assert_eq!(verify_exit_code(Ok(Status::CheckFailed)), 1);
        assert_eq!(verify_exit_code(Ok(Status::OperationalError)), 2);
    }

    #[test]
    fn verify_exit_code_treats_setup_errors_as_operational() {
        assert_eq!(
            verify_exit_code(Err(eyre::eyre!("restic_password missing"))),
            2
        );
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

    #[test]
    fn check_remote_unit_exists_asks_systemctl_by_unit_file_name() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "UNIT FILE          STATE\nnavidrome.service  enabled\n",
        ));
        assert!(check_remote_unit_exists(&mock, "navidrome").unwrap());
        assert_eq!(
            mock.calls(),
            vec![crate::services::ssh::SshOp::RunRaw(vec![
                "systemctl".to_string(),
                "list-unit-files".to_string(),
                "navidrome.service".to_string(),
            ])]
        );
    }

    #[test]
    fn check_remote_unit_exists_is_false_when_systemctl_lists_nothing() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "0 unit files listed.\n",
        ));
        assert!(!check_remote_unit_exists(&mock, "navidrome").unwrap());
    }

    /// The case #693 is about: an unreachable Host must not read as an absent
    /// unit. Asserted on the whole chain, since the unit context wraps the
    /// transport wording the seam raises.
    #[test]
    fn check_remote_unit_exists_reports_a_transport_failure_as_one() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::transport_failure(
            "ssh: connect to host 10.0.0.9 port 22: Connection refused",
        ));

        let err = format!(
            "{:#}",
            check_remote_unit_exists(&mock, "navidrome").unwrap_err()
        );
        assert!(err.contains("navidrome.service"), "{err}");
        assert!(err.contains("Connection refused"), "{err}");
        assert!(
            !err.to_lowercase().contains("not found"),
            "must not read as an absent unit: {err}"
        );
    }

    /// systemd's exit code for a name it does not know is version-dependent
    /// (1 on 258), so a non-zero exit alongside an answer stays an answer.
    #[test]
    fn check_remote_unit_exists_reads_systemds_answer_under_a_nonzero_exit() {
        let absent = crate::services::ssh::MockSshSession::new();
        absent.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: b"UNIT FILE STATE PRESET\n\n0 unit files listed.\n".to_vec(),
            stderr: Vec::new(),
        });
        assert!(!check_remote_unit_exists(&absent, "navidrome").unwrap());

        let present = crate::services::ssh::MockSshSession::new();
        present.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: b"navidrome.service enabled enabled\n".to_vec(),
            stderr: Vec::new(),
        });
        assert!(
            check_remote_unit_exists(&present, "navidrome").unwrap(),
            "a listed unit is installed whatever systemd exited with"
        );
    }

    #[test]
    fn check_remote_unit_exists_keeps_a_timer_a_timer() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "bichon-archive.timer  enabled\n",
        ));
        assert!(check_remote_unit_exists(&mock, "bichon-archive.timer").unwrap());
    }

    #[test]
    fn check_remote_disk_space_converts_dfs_kilobytes_to_bytes() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "  20971520\n",
        ));
        assert_eq!(
            check_remote_disk_space(&mock, "/").unwrap(),
            20_971_520 * 1024
        );
        assert_eq!(
            mock.calls(),
            vec![crate::services::ssh::SshOp::Run(
                "df --output=avail / | tail -1".to_string()
            )]
        );
    }

    #[test]
    fn check_remote_disk_space_fails_on_unparsable_output() {
        let mock = crate::services::ssh::MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "df: /nope: No such file\n",
        ));
        assert!(check_remote_disk_space(&mock, "/nope").is_err());
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
            tailscale_ip: None,
            tailnet_tag: None,
            prefer_tailnet: false,
            unknown: toml::Table::new(),
        }
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
    fn resolve_timestamp_dir_latest_picks_newest() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("2026-03-01_10-00-00")).unwrap();
        fs::create_dir(tmp.path().join("2026-03-09_14-30-00")).unwrap();

        let dir = resolve_timestamp_dir(tmp.path(), "latest").unwrap();
        assert_eq!(dir, tmp.path().join("2026-03-09_14-30-00"));
    }

    #[test]
    fn resolve_timestamp_dir_latest_errors_without_timestamps() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_timestamp_dir(tmp.path(), "latest")
            .unwrap_err()
            .to_string();
        assert!(err.contains("No backup timestamps found"), "{err}");
    }

    #[test]
    fn resolve_timestamp_dir_joins_specific_id() {
        let tmp = tempfile::tempdir().unwrap();
        let ts = tmp.path().join("2026-03-09_14-30-00");
        fs::create_dir(&ts).unwrap();

        let dir = resolve_timestamp_dir(tmp.path(), "2026-03-09_14-30-00").unwrap();
        assert_eq!(dir, ts);
    }

    #[test]
    fn resolve_timestamp_dir_errors_on_missing_id() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_timestamp_dir(tmp.path(), "2026-01-01_00-00-00")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Backup not found"), "{err}");
    }

    #[test]
    fn list_restorable_apps_returns_sorted_app_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("navidrome")).unwrap();
        fs::create_dir(tmp.path().join("actual")).unwrap();
        fs::create_dir(tmp.path().join("baikal")).unwrap();
        fs::write(tmp.path().join("stray-file.txt"), b"not an app").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("baikal"), tmp.path().join("linked")).unwrap();

        let apps = list_restorable_apps(tmp.path()).unwrap();
        assert_eq!(apps, vec!["actual", "baikal", "navidrome"]);
    }

    #[test]
    fn test_resolve_backup_dir_literal_latest_resolves_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        fs::create_dir(host_dir.join("2026-03-01_10-00-00")).unwrap();
        fs::create_dir(host_dir.join("2026-03-09_14-30-00")).unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("myserver"), Some("latest")).unwrap();
        assert_eq!(result, host_dir.join("2026-03-09_14-30-00"));
    }

    #[test]
    fn list_restorable_apps_is_empty_for_empty_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let apps = list_restorable_apps(tmp.path()).unwrap();
        assert!(apps.is_empty());
    }

    #[test]
    fn select_restore_apps_errors_when_backup_holds_no_apps() {
        let tmp = tempfile::tempdir().unwrap();
        let err = select_restore_apps(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("No app backups found"), "{err}");
    }

    #[test]
    fn select_restore_apps_errors_in_non_interactive_mode() {
        // A destructive restore must never fall back to an implicit app
        // subset, so without a TTY the guard demands -a instead.
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("navidrome")).unwrap();

        let err = select_restore_apps(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("non-interactive mode"), "{err}");
        assert!(err.contains("-a"), "{err}");
    }

    #[test]
    fn select_backup_id_errors_in_non_interactive_mode() {
        // Tests run without a TTY, so the non-interactive guard fires first
        // regardless of directory contents.
        let tmp = tempfile::tempdir().unwrap();
        let result = select_backup_id(tmp.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("non-interactive mode"),
        );
    }
}
