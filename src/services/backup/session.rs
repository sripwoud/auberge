use crate::config::Config;
use crate::hosts::Host;
use crate::output;
use crate::playbook_meta::BackupRecipe;
use crate::services::backup::executor::RecipeExecutor;
use crate::services::backup::restic::{ResticMessage, parse_restic_message};
use crate::services::backup::ssh::SshSession;
use crate::services::progress::Progress;
use chrono::Utc;
use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tabled::Tabled;

#[derive(Debug, Clone)]
pub struct CreateOutcome {
    pub successful_apps: Vec<String>,
    pub failed_apps: Vec<(String, String)>,
    pub timestamp: String,
}

/// Orchestrates multiple `RecipeExecutor` invocations across a `Host`'s apps. Owns
/// cross-recipe concerns (timestamp layout, per-app staging dir creation, result
/// aggregation); per-recipe semantics live in `RecipeExecutor`.
pub struct BackupSession<S: SshSession> {
    session: S,
    host: Host,
    apps: Vec<(String, BackupRecipe)>,
    parameters: HashMap<String, bool>,
    backup_root: PathBuf,
}

impl<S: SshSession> BackupSession<S> {
    pub fn new(
        session: S,
        host: Host,
        apps: Vec<(String, BackupRecipe)>,
        parameters: HashMap<String, bool>,
        backup_root: PathBuf,
    ) -> Self {
        Self {
            session,
            host,
            apps,
            parameters,
            backup_root,
        }
    }

    /// Backs up every app in `self.apps`, returning a [`CreateOutcome`] that captures
    /// which apps succeeded, which failed, and the shared timestamp directory.
    pub fn create(&self, progress: &mut dyn Progress) -> Result<CreateOutcome> {
        let start_time = Instant::now();
        let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

        let mut results: Vec<(String, bool, Option<u64>, Option<String>)> = Vec::new();

        for (app_name, recipe) in &self.apps {
            let app_backup_dir = self
                .backup_root
                .join(&self.host.name)
                .join(&timestamp)
                .join(app_name);

            if let Err(e) = fs::create_dir_all(&app_backup_dir) {
                eprintln!("✗ {} backup failed: {}", app_name, e);
                results.push((app_name.clone(), false, None, Some(e.to_string())));
                continue;
            }

            let executor = RecipeExecutor::new(&self.session);
            match executor.backup(recipe, &app_backup_dir, &self.parameters, progress) {
                Ok(()) => {
                    let size = calculate_dir_size(&app_backup_dir).unwrap_or(0);
                    if !output::is_verbose() {
                        output::success(&format!(
                            "{} ({})",
                            app_name,
                            output::format_size(size)
                        ));
                    }
                    results.push((app_name.clone(), true, Some(size), None));
                }
                Err(e) => {
                    eprintln!("✗ {} backup failed: {}", app_name, e);
                    let _ = fs::remove_dir_all(&app_backup_dir);
                    results.push((app_name.clone(), false, None, Some(e.to_string())));
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
                self.backup_root.join(&self.host.name).display(),
                timestamp
            ));
        }

        Ok(CreateOutcome {
            successful_apps: results
                .iter()
                .filter(|(_, ok, _, _)| *ok)
                .map(|(name, _, _, _)| name.clone())
                .collect(),
            failed_apps: results
                .into_iter()
                .filter(|(_, ok, _, _)| !*ok)
                .map(|(name, _, _, err)| (name, err.unwrap_or_default()))
                .collect(),
            timestamp,
        })
    }

    /// Orchestrates the full sync pipeline: create → push → prune → cleanup.
    ///
    /// When `dry_run` is `true` the create step is skipped and an informational
    /// message is emitted instead.
    pub fn run_sync(&self, dry_run: bool, progress: &mut dyn Progress) -> Result<()> {
        self.run_sync_impl(dry_run, progress, push_to_restic, prune_restic)
    }

    /// Internal implementation that accepts injectable push/prune functions for
    /// testing without a live restic binary.
    pub(crate) fn run_sync_impl<FP, FPR>(
        &self,
        dry_run: bool,
        progress: &mut dyn Progress,
        push_fn: FP,
        prune_fn: FPR,
    ) -> Result<()>
    where
        FP: Fn(&Path, &mut dyn Progress) -> Result<()>,
        FPR: Fn(bool, &mut dyn Progress) -> Result<()>,
    {
        if dry_run {
            output::info("Dry run: would next push to restic, prune, and clean up local staging");
            return Ok(());
        }

        let outcome = self.create(progress)?;

        let staging_dir = self
            .backup_root
            .join(&self.host.name)
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

        push_fn(&staging_dir, progress)?;

        if let Err(e) = prune_fn(false, progress) {
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
}

/// Push the local backup directory at `backup_dir` to the configured restic repository.
pub fn push_to_restic(backup_dir: &Path, progress: &mut dyn Progress) -> Result<()> {
    let (restic_repo, restic_password) = load_restic_config()?;

    output::info(&format!("Pushing {} to restic", backup_dir.display()));

    progress.task_started("Checking restic repository");
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
        progress.task_started("Initializing restic repository");
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

    progress.task_started(&format!("Pushing {}", backup_dir.display()));
    let mut snapshot_id: Option<String> = None;

    let result = output::stream_command_stdout(
        "restic",
        Command::new("restic")
            .arg("backup")
            .arg("--json")
            .arg(backup_dir)
            .env("RESTIC_REPOSITORY", &restic_repo)
            .env("RESTIC_PASSWORD", &restic_password)
            .env_remove("RESTIC_PASSWORD_COMMAND"),
        |line| match parse_restic_message(line) {
            Some(ResticMessage::Status(s)) => {
                if let (Some(total), Some(done)) = (s.total_bytes, s.bytes_done) {
                    progress.set_total(Some(total));
                    progress.bytes_transferred(done);
                } else {
                    progress.set_total(Some(100));
                    progress.bytes_transferred((s.percent_done * 100.0) as u64);
                }
            }
            Some(ResticMessage::Summary(s)) => {
                snapshot_id = Some(s.snapshot_id);
            }
            None => {}
        },
    )
    .wrap_err("Failed to run restic backup")?;

    progress.task_done();

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

/// Apply the retention policy to the restic repository (7 daily, 4 weekly, 12 monthly).
pub fn prune_restic(dry_run: bool, progress: &mut dyn Progress) -> Result<()> {
    let (restic_repo, restic_password) = load_restic_config()?;

    progress.task_started("Pruning restic snapshots");

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

    progress.task_done();

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

/// Remove the local staging directory after a successful push.
pub fn cleanup_staging_dir(staging_dir: &Path) -> Result<()> {
    fs::remove_dir_all(staging_dir)
        .wrap_err_with(|| format!("Failed to clean up staging dir: {}", staging_dir.display()))?;
    output::success(&format!(
        "Cleaned up local staging ({})",
        staging_dir.display()
    ));
    Ok(())
}

/// Return the default local backup root (`$XDG_DATA_HOME/auberge/backups`).
pub fn default_backup_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("auberge").join("backups"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/auberge/backups"))
}

/// Resolve the concrete backup directory from a root, optional host filter, and
/// optional backup ID. When host or backup ID are omitted the most recent
/// available entry is selected (interactively if multiple hosts exist).
pub fn resolve_backup_dir(
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

/// Load restic repository URL and password from the application config.
pub fn load_restic_config() -> Result<(String, String)> {
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

/// Recursively sum the sizes of all regular files under `path`.
pub(crate) fn calculate_dir_size(path: &Path) -> Result<u64> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::Host;
    use crate::playbook_meta::BackupRecipe;
    use crate::services::backup::ssh::{CommandResult, MockSshSession, SshOp};
    use crate::services::progress::{MockProgress, ProgressEvent};
    use std::collections::HashMap;

    fn make_host() -> Host {
        Host {
            name: "test-host".to_string(),
            address: "192.0.2.1".to_string(),
            user: "deploy".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
        }
    }

    fn simple_recipe(paths: Vec<&str>) -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec![],
            paths: paths.into_iter().map(|s| s.to_string()).collect(),
            owner: None,
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
        }
    }

    fn make_session(
        host: Host,
        recipes: Vec<(&str, BackupRecipe)>,
        mock: MockSshSession,
        backup_root: PathBuf,
    ) -> BackupSession<MockSshSession> {
        let apps = recipes
            .into_iter()
            .map(|(name, recipe)| (name.to_string(), recipe))
            .collect();
        BackupSession::new(mock, host, apps, HashMap::new(), backup_root)
    }

    // -------------------------------------------------------------------------
    // create() — happy path
    // -------------------------------------------------------------------------

    #[test]
    fn create_happy_path_returns_successful_outcome() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();
        let bs = make_session(
            host,
            vec![("myapp", simple_recipe(vec!["/opt/myapp"]))],
            session,
            tmp.path().to_path_buf(),
        );

        let mut progress = MockProgress::new();
        let outcome = bs.create(&mut progress).unwrap();

        assert_eq!(outcome.successful_apps, vec!["myapp"]);
        assert!(outcome.failed_apps.is_empty());
        assert!(!outcome.timestamp.is_empty());
    }

    #[test]
    fn create_happy_path_emits_task_started_event() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();
        let bs = make_session(
            host,
            vec![("myapp", simple_recipe(vec!["/opt/myapp"]))],
            session,
            tmp.path().to_path_buf(),
        );

        let mut progress = MockProgress::new();
        bs.create(&mut progress).unwrap();

        // RecipeExecutor emits rsync task_started for each path
        let events = progress.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ProgressEvent::TaskStarted(s) if s.contains("/opt/myapp"))),
            "expected task_started for /opt/myapp in {:?}",
            events
        );
    }

    #[test]
    fn create_happy_path_records_ssh_rsync_call() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();
        let bs = make_session(
            host,
            vec![("myapp", simple_recipe(vec!["/opt/myapp"]))],
            session,
            tmp.path().to_path_buf(),
        );

        let mut progress = MockProgress::new();
        bs.create(&mut progress).unwrap();

        let calls = bs.session.calls();
        assert!(
            calls.iter().any(|c| matches!(c, SshOp::RsyncFrom { remote, .. } if remote == "/opt/myapp")),
            "expected RsyncFrom /opt/myapp in {:?}",
            calls
        );
    }

    // -------------------------------------------------------------------------
    // create() — partial failure path
    // -------------------------------------------------------------------------

    #[test]
    fn create_partial_failure_continues_remaining_apps() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();

        use crate::playbook_meta::DbRecipe;
        let failing_recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/opt/app1".to_string()],
            owner: None,
            db: Some(DbRecipe {
                name: "mydb".to_string(),
                dump_path: "/tmp/mydb.dump".to_string(),
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
        };
        // Stage pg_dump failure so app1 fails
        session.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: vec![],
            stderr: b"pg_dump: error".to_vec(),
        });

        let bs = make_session(
            host,
            vec![
                ("app1", failing_recipe),
                ("app2", simple_recipe(vec!["/opt/app2"])),
            ],
            session,
            tmp.path().to_path_buf(),
        );

        let mut progress = MockProgress::new();
        let outcome = bs.create(&mut progress).unwrap();

        assert_eq!(outcome.failed_apps.len(), 1, "app1 should fail");
        assert_eq!(outcome.failed_apps[0].0, "app1");
        assert_eq!(outcome.successful_apps, vec!["app2"]);
        assert!(!outcome.timestamp.is_empty());
    }

    // -------------------------------------------------------------------------
    // run_sync_impl() — coordination tests (injectable push/prune)
    // -------------------------------------------------------------------------

    #[test]
    fn run_sync_happy_path_calls_push_and_prune() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();
        let bs = make_session(
            host,
            vec![("myapp", simple_recipe(vec!["/opt/myapp"]))],
            session,
            tmp.path().to_path_buf(),
        );

        let push_called = std::cell::Cell::new(false);
        let prune_called = std::cell::Cell::new(false);

        let push_fn = |_dir: &Path, _p: &mut dyn Progress| -> Result<()> {
            push_called.set(true);
            Ok(())
        };
        let prune_fn = |_dry: bool, _p: &mut dyn Progress| -> Result<()> {
            prune_called.set(true);
            Ok(())
        };

        let mut progress = MockProgress::new();
        bs.run_sync_impl(false, &mut progress, push_fn, prune_fn)
            .unwrap();

        assert!(push_called.get(), "push should have been called");
        assert!(prune_called.get(), "prune should have been called");
    }

    #[test]
    fn run_sync_partial_failure_still_pushes_and_prunes_survivors() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();

        use crate::playbook_meta::DbRecipe;
        let failing_recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/opt/app1".to_string()],
            owner: None,
            db: Some(DbRecipe {
                name: "mydb".to_string(),
                dump_path: "/tmp/mydb.dump".to_string(),
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
        };
        // Stage pg_dump failure so app1 fails
        session.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: vec![],
            stderr: b"pg_dump: error".to_vec(),
        });

        let bs = make_session(
            host,
            vec![
                ("app1", failing_recipe),
                ("app2", simple_recipe(vec!["/opt/app2"])),
            ],
            session,
            tmp.path().to_path_buf(),
        );

        let push_called = std::cell::Cell::new(false);
        let prune_called = std::cell::Cell::new(false);

        let push_fn = |_dir: &Path, _p: &mut dyn Progress| -> Result<()> {
            push_called.set(true);
            Ok(())
        };
        let prune_fn = |_dry: bool, _p: &mut dyn Progress| -> Result<()> {
            prune_called.set(true);
            Ok(())
        };

        let mut progress = MockProgress::new();
        // run_sync_impl should fail (due to partial failure) but still push/prune
        let result = bs.run_sync_impl(false, &mut progress, push_fn, prune_fn);

        // Should return error because app1 failed
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("app failure"),
            "error should mention app failure"
        );
        // Push and prune should still have run on the surviving app2
        assert!(push_called.get(), "push should run despite app1 failure");
        assert!(prune_called.get(), "prune should run despite app1 failure");
    }

    #[test]
    fn run_sync_all_failed_does_not_call_push() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();

        use crate::playbook_meta::DbRecipe;
        let failing_recipe = BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/opt/app1".to_string()],
            owner: None,
            db: Some(DbRecipe {
                name: "mydb".to_string(),
                dump_path: "/tmp/mydb.dump".to_string(),
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
        };
        session.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: vec![],
            stderr: b"pg_dump: error".to_vec(),
        });

        let bs = make_session(
            host,
            vec![("app1", failing_recipe)],
            session,
            tmp.path().to_path_buf(),
        );

        let push_called = std::cell::Cell::new(false);

        let push_fn = |_dir: &Path, _p: &mut dyn Progress| -> Result<()> {
            push_called.set(true);
            Ok(())
        };
        let prune_fn = |_dry: bool, _p: &mut dyn Progress| -> Result<()> { Ok(()) };

        let mut progress = MockProgress::new();
        let result = bs.run_sync_impl(false, &mut progress, push_fn, prune_fn);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("nothing to push")
        );
        assert!(!push_called.get(), "push must NOT be called when all apps fail");
    }

    #[test]
    fn run_sync_dry_run_skips_create_and_push() {
        let tmp = tempfile::tempdir().unwrap();
        let host = make_host();
        let session = MockSshSession::new();
        let bs = make_session(
            host,
            vec![("myapp", simple_recipe(vec!["/opt/myapp"]))],
            session,
            tmp.path().to_path_buf(),
        );

        let push_called = std::cell::Cell::new(false);
        let push_fn = |_dir: &Path, _p: &mut dyn Progress| -> Result<()> {
            push_called.set(true);
            Ok(())
        };
        let prune_fn = |_dry: bool, _p: &mut dyn Progress| -> Result<()> { Ok(()) };

        let mut progress = MockProgress::new();
        bs.run_sync_impl(true, &mut progress, push_fn, prune_fn)
            .unwrap();

        assert!(!push_called.get(), "push must NOT be called for dry run");
        // No SSH calls should have been made
        assert!(bs.session.calls().is_empty());
    }

    // -------------------------------------------------------------------------
    // resolve_backup_dir()
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_backup_dir_empty_root_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_backup_dir(tmp.path(), None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No backups found"));
    }

    #[test]
    fn resolve_backup_dir_single_host_auto_selects() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        let ts_dir = host_dir.join("2026-03-09_14-30-00");
        fs::create_dir(&ts_dir).unwrap();

        let result = resolve_backup_dir(tmp.path(), None, None).unwrap();
        assert_eq!(result, ts_dir);
    }

    #[test]
    fn resolve_backup_dir_with_host_filter() {
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
    fn resolve_backup_dir_host_not_found_errors() {
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
    fn resolve_backup_dir_picks_latest_timestamp() {
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
    fn resolve_backup_dir_excludes_symlinks_and_non_timestamp_dirs() {
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
    fn resolve_backup_dir_specific_backup_id() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();
        let ts = host_dir.join("2026-03-09_14-30-00");
        fs::create_dir(&ts).unwrap();

        let result =
            resolve_backup_dir(tmp.path(), Some("myserver"), Some("2026-03-09_14-30-00"))
                .unwrap();
        assert_eq!(result, ts);
    }

    #[test]
    fn resolve_backup_dir_specific_backup_id_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let host_dir = tmp.path().join("myserver");
        fs::create_dir(&host_dir).unwrap();

        let result = resolve_backup_dir(tmp.path(), Some("myserver"), Some("2026-01-01_00-00-00"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Backup not found"));
    }

    #[test]
    fn resolve_backup_dir_selects_newest_for_cleanup() {
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

    // -------------------------------------------------------------------------
    // cleanup_staging_dir()
    // -------------------------------------------------------------------------

    #[test]
    fn cleanup_staging_dir_removes_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("2026-04-06_03-00-00");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("data.bin"), vec![0u8; 1024]).unwrap();

        assert!(staging.exists());
        cleanup_staging_dir(&staging).unwrap();
        assert!(!staging.exists());
    }

    #[test]
    fn cleanup_staging_dir_fails_on_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("nonexistent");
        assert!(cleanup_staging_dir(&staging).is_err());
    }

    // -------------------------------------------------------------------------
    // CreateOutcome
    // -------------------------------------------------------------------------

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
}
