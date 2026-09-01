use crate::output;
use crate::playbook_meta::BackupRecipe;
use crate::services::backup::executor::RecipeExecutor;
use crate::services::backup::restic::{self, ResticMessage, parse_restic_message};
use crate::services::progress::Progress;
use crate::services::ssh::SshSession;
use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SessionOpts {
    pub host_name: String,
    pub dest: PathBuf,
    pub timestamp: String,
    pub parameters: HashMap<String, bool>,
}

#[derive(Debug, Clone)]
pub struct RecipeOutcome {
    pub app: String,
    pub size_bytes: Option<u64>,
    pub error: Option<String>,
}

impl RecipeOutcome {
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct CreateOutcome {
    pub results: Vec<RecipeOutcome>,
    pub timestamp: String,
}

impl CreateOutcome {
    pub fn successful_apps(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|r| r.is_success())
            .map(|r| r.app.clone())
            .collect()
    }

    pub fn failed_apps(&self) -> Vec<(String, String)> {
        self.results
            .iter()
            .filter(|r| !r.is_success())
            .map(|r| (r.app.clone(), r.error.clone().unwrap_or_default()))
            .collect()
    }

    pub fn total_size(&self) -> u64 {
        self.results.iter().filter_map(|r| r.size_bytes).sum()
    }
}

/// A `Progress` per App, built by the caller.
///
/// One per App rather than one per Session: the terminal shows a bar for the
/// App being backed up, and a single shared Progress would collapse thirteen
/// of them into one (ADR-0047).
pub type ProgressFactory<'a> = Box<dyn Fn(&str) -> Box<dyn Progress> + 'a>;

pub struct BackupSession<'a, S: SshSession + ?Sized> {
    ssh: &'a S,
    recipes: Vec<(String, BackupRecipe)>,
    opts: SessionOpts,
    progress_for: ProgressFactory<'a>,
}

impl<'a, S: SshSession + ?Sized> BackupSession<'a, S> {
    pub fn new(
        ssh: &'a S,
        recipes: Vec<(String, BackupRecipe)>,
        opts: SessionOpts,
        progress_for: impl Fn(&str) -> Box<dyn Progress> + 'a,
    ) -> Self {
        Self {
            ssh,
            recipes,
            opts,
            progress_for: Box::new(progress_for),
        }
    }

    pub fn create(&self) -> Result<CreateOutcome> {
        let executor = RecipeExecutor::new(self.ssh);
        let mut results = Vec::with_capacity(self.recipes.len());

        for (app_name, recipe) in &self.recipes {
            let mut progress = (self.progress_for)(app_name);
            let app_dir = self
                .opts
                .dest
                .join(&self.opts.host_name)
                .join(&self.opts.timestamp)
                .join(app_name);

            if let Err(e) = fs::create_dir_all(&app_dir) {
                progress.error(&format!("{} backup failed: {}", app_name, e));
                results.push(RecipeOutcome {
                    app: app_name.clone(),
                    size_bytes: None,
                    error: Some(e.to_string()),
                });
                continue;
            }

            let exec_result = executor.backup(
                app_name,
                recipe,
                &app_dir,
                &self.opts.parameters,
                &mut *progress,
            );

            match exec_result {
                Ok(()) => {
                    let size = calculate_dir_size(&app_dir).unwrap_or(0);
                    progress.success(&format!("{} ({})", app_name, output::format_size(size)));
                    results.push(RecipeOutcome {
                        app: app_name.clone(),
                        size_bytes: Some(size),
                        error: None,
                    });
                }
                Err(e) => {
                    let _ = fs::remove_dir_all(&app_dir);
                    progress.error(&format!("{} backup failed: {}", app_name, e));
                    results.push(RecipeOutcome {
                        app: app_name.clone(),
                        size_bytes: None,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(CreateOutcome {
            results,
            timestamp: self.opts.timestamp.clone(),
        })
    }
}

fn backup_args(backup_dir: &Path, host: &str) -> Vec<std::ffi::OsString> {
    vec![
        "backup".into(),
        "--json".into(),
        "--tag".into(),
        host.into(),
        backup_dir.into(),
    ]
}

fn forget_args(dry_run: bool) -> Vec<&'static str> {
    let mut args = vec![
        "forget",
        "--group-by",
        "tags",
        "--keep-daily",
        "7",
        "--keep-weekly",
        "4",
        "--keep-monthly",
        "12",
        "--prune",
    ];
    if dry_run {
        args.push("--dry-run");
    }
    args
}

/// Whether restic's repository already exists at the configured location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoState {
    Initialized,
    Uninitialized,
}

pub fn restic_push(
    restic_repo: &str,
    restic_password: &str,
    backup_dir: &Path,
    host: &str,
    progress: &mut dyn Progress,
) -> Result<()> {
    drive_restic_push(
        backup_dir,
        progress,
        || probe_repo(restic_repo, restic_password),
        || init_repo(restic_repo, restic_password),
        |progress| stream_push(restic_repo, restic_password, backup_dir, host, progress),
    )
}

/// The push's ordering and its whole output, over closures that stand for the
/// three restic invocations.
///
/// Split out so both are assertable without restic: what a run reports is the
/// event stream, and the branch that decides whether the repository is created
/// first is otherwise reachable only against a repository that does not exist.
fn drive_restic_push(
    backup_dir: &Path,
    progress: &mut dyn Progress,
    probe: impl FnOnce() -> Result<RepoState>,
    init: impl FnOnce() -> Result<()>,
    push: impl FnOnce(&mut dyn Progress) -> Result<Option<String>>,
) -> Result<()> {
    progress.info(&format!("Pushing {} to restic", backup_dir.display()));
    progress.task_started("Checking restic repository");

    if probe()? == RepoState::Uninitialized {
        progress.task_started("Initializing restic repository");
        init()?;
    }

    progress.task_started(&format!("Pushing {}", backup_dir.display()));
    let pushed = push(&mut *progress);
    progress.task_done();

    match pushed? {
        Some(id) => progress.success(&format!("Push complete: snapshot {}", id)),
        None => progress.success("Push complete"),
    }

    Ok(())
}

fn probe_repo(restic_repo: &str, restic_password: &str) -> Result<RepoState> {
    let snapshots_check = restic::command(restic_repo, restic_password)
        .arg("snapshots")
        .arg("--json")
        .output();

    match snapshots_check {
        Ok(out) => {
            let stderr_text = String::from_utf8_lossy(&out.stderr);
            let lines = output::subprocess_output("restic", &stderr_text);
            if out.status.success() {
                output::clear_subprocess_lines(lines);
                Ok(RepoState::Initialized)
            } else if stderr_text.contains("Is there a repository at the following location")
                || stderr_text.contains("unable to open config file")
            {
                output::clear_subprocess_lines(lines);
                Ok(RepoState::Uninitialized)
            } else {
                eyre::bail!("restic snapshots failed: {}", stderr_text.trim());
            }
        }
        Err(_) => eyre::bail!("restic not found. Install restic: https://restic.net"),
    }
}

fn init_repo(restic_repo: &str, restic_password: &str) -> Result<()> {
    let init_output = restic::command(restic_repo, restic_password)
        .arg("init")
        .output()
        .wrap_err("Failed to initialize restic repository")?;
    let stderr_text = String::from_utf8_lossy(&init_output.stderr);
    let lines = output::subprocess_output("restic", &stderr_text);

    if !init_output.status.success() {
        eyre::bail!(
            "Failed to initialize restic repository: {}",
            stderr_text.trim()
        );
    }
    output::clear_subprocess_lines(lines);

    Ok(())
}

/// Runs `restic backup`, reporting bytes as they land. `Ok(None)` is a
/// successful push whose summary message never arrived.
fn stream_push(
    restic_repo: &str,
    restic_password: &str,
    backup_dir: &Path,
    host: &str,
    progress: &mut dyn Progress,
) -> Result<Option<String>> {
    let mut snapshot_id: Option<String> = None;

    let result = output::stream_command_stdout(
        "restic",
        restic::command(restic_repo, restic_password).args(backup_args(backup_dir, host)),
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
            // restic reports failures on stderr, surfaced via `result.status` below.
            Some(ResticMessage::ExitError(_)) | None => {}
        },
    )
    .wrap_err("Failed to run restic backup")?;

    if !result.status.success() {
        if result.last_stderr.is_empty() {
            eyre::bail!("restic backup failed");
        }
        eyre::bail!("restic backup failed: {}", result.last_stderr.trim());
    }

    Ok(snapshot_id)
}

pub fn restic_prune(
    restic_repo: &str,
    restic_password: &str,
    dry_run: bool,
    progress: &mut dyn Progress,
) -> Result<()> {
    drive_restic_prune(dry_run, progress, || {
        forget_snapshots(restic_repo, restic_password, dry_run)
    })
}

fn drive_restic_prune(
    dry_run: bool,
    progress: &mut dyn Progress,
    forget: impl FnOnce() -> Result<String>,
) -> Result<()> {
    progress.task_started("Pruning restic snapshots");
    let report = forget();
    progress.task_done();

    let report = report?;
    let report = report.trim();
    if !report.is_empty() {
        progress.line(report);
    }

    if dry_run {
        progress.info("Dry run completed (no changes made)");
    } else {
        progress.success("Prune complete");
    }

    Ok(())
}

/// Runs `restic forget --prune`, returning restic's own retention report.
fn forget_snapshots(restic_repo: &str, restic_password: &str, dry_run: bool) -> Result<String> {
    let mut cmd = restic::command(restic_repo, restic_password);
    cmd.args(forget_args(dry_run));

    let prune_output = cmd.output().wrap_err("Failed to run restic forget")?;

    let stderr_text = String::from_utf8_lossy(&prune_output.stderr);
    let lines = output::subprocess_output("restic", &stderr_text);

    if !prune_output.status.success() {
        eyre::bail!("restic prune failed: {}", stderr_text.trim());
    }
    output::clear_subprocess_lines(lines);

    Ok(String::from_utf8_lossy(&prune_output.stdout).into_owned())
}

pub fn calculate_dir_size(path: &Path) -> Result<u64> {
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
    use crate::playbook_meta::{DbEngine, DbRecipe};
    use crate::services::progress::{MockProgress, ProgressEvent};
    use crate::services::ssh::{MockSshSession, SshOp};
    use std::cell::RefCell;

    /// A factory that records every App's events into one list, in the order
    /// the Session emitted them.
    fn recording(into: &MockProgress) -> impl Fn(&str) -> Box<dyn Progress> + '_ {
        move |_app| Box::new(into.share())
    }

    fn baikal_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/opt/baikal/Specific".to_string()],
            owner: Some(("baikal".to_string(), "baikal".to_string())),
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        }
    }

    fn bichon_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec!["bichon".to_string()],
            paths: vec!["/opt/bichon/data".to_string()],
            owner: None,
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        }
    }

    fn paperless_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec!["paperless-webserver".to_string()],
            paths: vec!["/opt/paperless/data".to_string()],
            owner: Some(("paperless".to_string(), "paperless".to_string())),
            db: Some(DbRecipe {
                name: "paperless".to_string(),
                dump_path: "/tmp/paperless_db.dump".to_string(),
                engine: DbEngine::Postgres,
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        }
    }

    fn opts(dest: &Path) -> SessionOpts {
        SessionOpts {
            host_name: "myserver".to_string(),
            dest: dest.to_path_buf(),
            timestamp: "2026-04-28_03-00-00".to_string(),
            parameters: HashMap::new(),
        }
    }

    #[test]
    fn backup_args_tags_snapshot_with_host() {
        let args = backup_args(
            Path::new("/backups/myserver/2026-04-28_03-00-00"),
            "myserver",
        );

        let tag_pos = args.iter().position(|a| a == "--tag").unwrap();
        assert_eq!(args[tag_pos + 1], "myserver");
        assert_eq!(
            args.last().unwrap(),
            &std::ffi::OsString::from("/backups/myserver/2026-04-28_03-00-00")
        );
    }

    #[test]
    fn forget_args_group_by_tags_so_retention_spans_snapshots() {
        let args = forget_args(false);

        assert!(args.windows(2).any(|w| w == ["--group-by", "tags"]));
        assert!(args.windows(2).any(|w| w == ["--keep-daily", "7"]));
        assert!(args.contains(&"--prune"));
        assert!(!args.contains(&"--dry-run"));
    }

    #[test]
    fn forget_args_dry_run_appends_flag() {
        assert!(forget_args(true).contains(&"--dry-run"));
    }

    fn push_dir() -> &'static Path {
        Path::new("/backups/myserver/2026-04-28_03-00-00")
    }

    #[test]
    fn push_initializes_an_absent_repository_before_pushing() {
        let mut progress = MockProgress::new();
        let mut initialized = false;

        drive_restic_push(
            push_dir(),
            &mut progress,
            || Ok(RepoState::Uninitialized),
            || {
                initialized = true;
                Ok(())
            },
            |_| Ok(Some("abc123".to_string())),
        )
        .unwrap();

        assert!(initialized);
        assert_eq!(
            progress.events(),
            [
                ProgressEvent::Info(
                    "Pushing /backups/myserver/2026-04-28_03-00-00 to restic".to_string()
                ),
                ProgressEvent::TaskStarted("Checking restic repository".to_string()),
                ProgressEvent::TaskStarted("Initializing restic repository".to_string()),
                ProgressEvent::TaskStarted(
                    "Pushing /backups/myserver/2026-04-28_03-00-00".to_string()
                ),
                ProgressEvent::TaskDone,
                ProgressEvent::Success("Push complete: snapshot abc123".to_string()),
            ]
        );
    }

    #[test]
    fn push_leaves_an_initialized_repository_alone() {
        let mut progress = MockProgress::new();

        drive_restic_push(
            push_dir(),
            &mut progress,
            || Ok(RepoState::Initialized),
            || panic!("an initialized repository must not be re-initialized"),
            |_| Ok(Some("abc123".to_string())),
        )
        .unwrap();

        assert!(!progress.events().contains(&ProgressEvent::TaskStarted(
            "Initializing restic repository".to_string()
        )));
    }

    // restic reports the snapshot id in a summary message; a push that lands
    // without one still succeeded, and says so without inventing an id.
    #[test]
    fn push_reports_completion_when_no_snapshot_id_arrives() {
        let mut progress = MockProgress::new();

        drive_restic_push(
            push_dir(),
            &mut progress,
            || Ok(RepoState::Initialized),
            || Ok(()),
            |_| Ok(None),
        )
        .unwrap();

        assert_eq!(
            progress.events().last(),
            Some(&ProgressEvent::Success("Push complete".to_string()))
        );
    }

    #[test]
    fn push_forwards_the_bytes_restic_reports() {
        let mut progress = MockProgress::new();

        drive_restic_push(
            push_dir(),
            &mut progress,
            || Ok(RepoState::Initialized),
            || Ok(()),
            |progress| {
                progress.set_total(Some(9_300_000_000));
                progress.bytes_transferred(1_200_000_000);
                Ok(Some("abc123".to_string()))
            },
        )
        .unwrap();

        let events = progress.events();
        assert!(events.contains(&ProgressEvent::SetTotal(Some(9_300_000_000))));
        assert!(events.contains(&ProgressEvent::BytesTransferred(1_200_000_000)));
    }

    #[test]
    fn push_that_fails_announces_no_completion() {
        let mut progress = MockProgress::new();

        let err = drive_restic_push(
            push_dir(),
            &mut progress,
            || Ok(RepoState::Initialized),
            || Ok(()),
            |_| eyre::bail!("restic backup failed: no space left on device"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("no space left on device"));
        let events = progress.events();
        assert_eq!(events.last(), Some(&ProgressEvent::TaskDone));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ProgressEvent::Success(_)))
        );
    }

    #[test]
    fn push_stops_before_pushing_when_the_repository_cannot_be_read() {
        let mut progress = MockProgress::new();

        let err = drive_restic_push(
            push_dir(),
            &mut progress,
            || eyre::bail!("restic snapshots failed: permission denied"),
            || panic!("an unreadable repository must not be initialized"),
            |_| panic!("an unreadable repository must not be pushed to"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("permission denied"));

        assert_eq!(
            progress.events(),
            [
                ProgressEvent::Info(
                    "Pushing /backups/myserver/2026-04-28_03-00-00 to restic".to_string()
                ),
                ProgressEvent::TaskStarted("Checking restic repository".to_string()),
            ]
        );
    }

    #[test]
    fn prune_reports_restics_own_retention_report() {
        let mut progress = MockProgress::new();

        drive_restic_prune(false, &mut progress, || {
            Ok("Applying Policy: keep 7 daily snapshots\nremove 2 snapshots\n".to_string())
        })
        .unwrap();

        assert_eq!(
            progress.events(),
            [
                ProgressEvent::TaskStarted("Pruning restic snapshots".to_string()),
                ProgressEvent::TaskDone,
                ProgressEvent::Line(
                    "Applying Policy: keep 7 daily snapshots\nremove 2 snapshots".to_string()
                ),
                ProgressEvent::Success("Prune complete".to_string()),
            ]
        );
    }

    #[test]
    fn prune_dry_run_reports_that_nothing_changed() {
        let mut progress = MockProgress::new();

        drive_restic_prune(true, &mut progress, || Ok("remove 2 snapshots".to_string())).unwrap();

        let events = progress.events();
        assert_eq!(
            events.last(),
            Some(&ProgressEvent::Info(
                "Dry run completed (no changes made)".to_string()
            ))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ProgressEvent::Success(_)))
        );
    }

    #[test]
    fn prune_with_nothing_to_report_emits_no_line() {
        let mut progress = MockProgress::new();

        drive_restic_prune(false, &mut progress, || Ok("  \n".to_string())).unwrap();

        assert_eq!(
            progress.events(),
            [
                ProgressEvent::TaskStarted("Pruning restic snapshots".to_string()),
                ProgressEvent::TaskDone,
                ProgressEvent::Success("Prune complete".to_string()),
            ]
        );
    }

    #[test]
    fn prune_that_fails_announces_no_completion() {
        let mut progress = MockProgress::new();

        let err = drive_restic_prune(false, &mut progress, || {
            eyre::bail!("restic prune failed: repository is locked")
        })
        .unwrap_err();

        assert!(err.to_string().contains("repository is locked"));
        assert_eq!(
            progress.events(),
            [
                ProgressEvent::TaskStarted("Pruning restic snapshots".to_string()),
                ProgressEvent::TaskDone,
            ]
        );
    }

    #[test]
    fn create_runs_recipes_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let recipes = vec![
            ("baikal".to_string(), baikal_recipe()),
            ("bichon".to_string(), bichon_recipe()),
        ];
        let events = MockProgress::new();
        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), recording(&events));

        let outcome = session.create().unwrap();

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results[0].app, "baikal");
        assert_eq!(outcome.results[1].app, "bichon");
        assert!(outcome.results.iter().all(RecipeOutcome::is_success));

        let calls = mock.calls();
        let baikal_rsync = calls.iter().position(
            |c| matches!(c, SshOp::RsyncFrom { remote, .. } if remote == "/opt/baikal/Specific"),
        );
        let bichon_stop = calls.iter().position(|c| {
            matches!(
                c,
                SshOp::Systemctl { action, service }
                if action == "stop" && service == "bichon"
            )
        });
        assert!(baikal_rsync.is_some());
        assert!(bichon_stop.is_some());
        assert!(baikal_rsync.unwrap() < bichon_stop.unwrap());
    }

    #[test]
    fn create_creates_per_app_dest_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let recipes = vec![
            ("baikal".to_string(), baikal_recipe()),
            ("bichon".to_string(), bichon_recipe()),
        ];
        let events = MockProgress::new();
        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), recording(&events));

        session.create().unwrap();

        let baikal_dir = tmp
            .path()
            .join("myserver")
            .join("2026-04-28_03-00-00")
            .join("baikal");
        let bichon_dir = tmp
            .path()
            .join("myserver")
            .join("2026-04-28_03-00-00")
            .join("bichon");
        assert!(baikal_dir.is_dir());
        assert!(bichon_dir.is_dir());
    }

    #[test]
    fn create_does_not_abort_on_recipe_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        // paperless is guarded (it declares a systemd service), so its
        // deadman's fire-check is the first `run()` call; stage its
        // "no marker" answer before the pg_dump failure this test is about.
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"connection refused".to_vec(),
        });

        let recipes = vec![
            ("paperless".to_string(), paperless_recipe()),
            ("baikal".to_string(), baikal_recipe()),
        ];
        let events = MockProgress::new();
        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), recording(&events));

        let outcome = session.create().unwrap();

        assert_eq!(outcome.results.len(), 2);
        let paperless = outcome
            .results
            .iter()
            .find(|r| r.app == "paperless")
            .unwrap();
        let baikal = outcome.results.iter().find(|r| r.app == "baikal").unwrap();
        assert!(!paperless.is_success());
        assert!(baikal.is_success());

        // Ensure the failed recipe's dest dir was cleaned up.
        let paperless_dir = tmp
            .path()
            .join("myserver")
            .join("2026-04-28_03-00-00")
            .join("paperless");
        assert!(!paperless_dir.exists());
    }

    #[test]
    fn create_outcome_helpers_partition_results() {
        let outcome = CreateOutcome {
            timestamp: "2026-04-28_03-00-00".to_string(),
            results: vec![
                RecipeOutcome {
                    app: "baikal".to_string(),
                    size_bytes: Some(1024),
                    error: None,
                },
                RecipeOutcome {
                    app: "bichon".to_string(),
                    size_bytes: None,
                    error: Some("oops".to_string()),
                },
                RecipeOutcome {
                    app: "freshrss".to_string(),
                    size_bytes: Some(2048),
                    error: None,
                },
            ],
        };

        assert_eq!(
            outcome.successful_apps(),
            vec!["baikal".to_string(), "freshrss".to_string()]
        );
        assert_eq!(
            outcome.failed_apps(),
            vec![("bichon".to_string(), "oops".to_string())]
        );
        assert_eq!(outcome.total_size(), 3072);
    }

    #[test]
    fn create_handles_empty_recipe_list() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let events = MockProgress::new();
        let session = BackupSession::new(&mock, vec![], opts(tmp.path()), recording(&events));

        let outcome = session.create().unwrap();

        assert!(outcome.results.is_empty());
        assert_eq!(outcome.timestamp, "2026-04-28_03-00-00");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn create_asks_for_one_progress_per_app_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let recipes = vec![
            ("baikal".to_string(), baikal_recipe()),
            ("bichon".to_string(), bichon_recipe()),
        ];
        let asked: RefCell<Vec<String>> = RefCell::new(Vec::new());

        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), |app| {
            asked.borrow_mut().push(app.to_string());
            Box::new(MockProgress::new())
        });
        session.create().unwrap();
        drop(session);

        assert_eq!(asked.into_inner(), vec!["baikal", "bichon"]);
    }

    #[test]
    fn create_reports_a_finished_app_as_a_success_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let events = MockProgress::new();
        let recipes = vec![("baikal".to_string(), baikal_recipe())];

        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), recording(&events));
        session.create().unwrap();

        assert!(
            events
                .events()
                .contains(&ProgressEvent::Success("baikal (0 B)".to_string()))
        );
    }

    #[test]
    fn create_reports_a_failed_app_as_an_error_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        // paperless is guarded, so its deadman's fire-check consumes the
        // first `run()` call; stage its "no marker" answer ahead of pg_dump's
        // failure this test is about.
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"connection refused".to_vec(),
        });
        let events = MockProgress::new();
        let recipes = vec![("paperless".to_string(), paperless_recipe())];

        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), recording(&events));
        session.create().unwrap();

        let reported: Vec<String> = events
            .events()
            .into_iter()
            .filter_map(|e| match e {
                ProgressEvent::Error(msg) => Some(msg),
                _ => None,
            })
            .collect();
        assert_eq!(reported.len(), 1);
        assert!(reported[0].starts_with("paperless backup failed: "));
        assert!(reported[0].contains("connection refused"));
    }

    // The per-App line has to stream, which is why it cannot move to the
    // summary the command renders after `create` returns.
    #[test]
    fn create_reports_each_app_before_the_next_one_starts() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let events = MockProgress::new();
        let recipes = vec![
            ("baikal".to_string(), baikal_recipe()),
            ("bichon".to_string(), bichon_recipe()),
        ];

        let session = BackupSession::new(&mock, recipes, opts(tmp.path()), recording(&events));
        session.create().unwrap();

        let stream = events.events();
        let baikal_done = stream
            .iter()
            .position(|e| matches!(e, ProgressEvent::Success(m) if m.starts_with("baikal ")))
            .unwrap();
        let bichon_started = stream
            .iter()
            .position(|e| matches!(e, ProgressEvent::TaskStarted(m) if m == "Stopping bichon"))
            .unwrap();
        assert!(baikal_done < bichon_started);
    }

    #[test]
    fn create_passes_parameters_to_executor() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();

        let mut params = HashMap::new();
        params.insert(
            "include_music".to_string(),
            crate::playbook_meta::BackupParameter {
                default: false,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        let navidrome = BackupRecipe {
            systemd_services: vec!["navidrome".to_string()],
            paths: vec!["/var/lib/navidrome".to_string()],
            owner: None,
            db: None,
            post_restore_command: None,
            parameters: params,
            attests: None,
            restore_advice: None,
        };

        let mut session_params = HashMap::new();
        session_params.insert("include_music".to_string(), true);

        let opts_with_param = SessionOpts {
            host_name: "myserver".to_string(),
            dest: tmp.path().to_path_buf(),
            timestamp: "2026-04-28_03-00-00".to_string(),
            parameters: session_params,
        };

        let events = MockProgress::new();
        let session = BackupSession::new(
            &mock,
            vec![("navidrome".to_string(), navidrome)],
            opts_with_param,
            recording(&events),
        );
        session.create().unwrap();

        let rsync_remotes: Vec<String> = mock
            .calls()
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncFrom { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect();
        assert!(rsync_remotes.contains(&"/srv/music".to_string()));
    }
}
