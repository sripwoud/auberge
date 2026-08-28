use crate::output;
use crate::playbook_meta::BackupRecipe;
use crate::services::backup::executor::RecipeExecutor;
use crate::services::backup::restic::{self, ResticMessage, parse_restic_message};
use crate::services::progress::{Progress, TerminalProgress};
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

            let exec_result =
                executor.backup(recipe, &app_dir, &self.opts.parameters, &mut *progress);

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

pub fn restic_push(
    restic_repo: &str,
    restic_password: &str,
    backup_dir: &Path,
    host: &str,
) -> Result<()> {
    output::info(&format!("Pushing {} to restic", backup_dir.display()));

    let mut progress = TerminalProgress::new("Checking restic repository");
    let snapshots_check = restic::command(restic_repo, restic_password)
        .arg("snapshots")
        .arg("--json")
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
        let init_output = restic::command(restic_repo, restic_password)
            .arg("init")
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

pub fn restic_prune(restic_repo: &str, restic_password: &str, dry_run: bool) -> Result<()> {
    let mut progress = TerminalProgress::new("Pruning restic snapshots");

    let mut cmd = restic::command(restic_repo, restic_password);
    cmd.args(forget_args(dry_run));

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
        // Stage a failure for paperless's pg_dump (the first run() call).
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
