use crate::output;
use crate::playbook_meta::BackupRecipe;
use crate::services::backup::executor::RecipeExecutor;
use crate::services::backup::restic::{ResticMessage, parse_restic_message};
use crate::services::backup::ssh::SshSession;
use crate::services::progress::{Progress, TerminalProgress};
use eyre::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub struct BackupSession<'a, S: SshSession + ?Sized> {
    ssh: &'a S,
    recipes: Vec<(String, BackupRecipe)>,
    opts: SessionOpts,
}

impl<'a, S: SshSession + ?Sized> BackupSession<'a, S> {
    pub fn new(ssh: &'a S, recipes: Vec<(String, BackupRecipe)>, opts: SessionOpts) -> Self {
        Self { ssh, recipes, opts }
    }

    pub fn create(&self) -> Result<CreateOutcome> {
        let executor = RecipeExecutor::new(self.ssh);
        let mut results = Vec::with_capacity(self.recipes.len());

        for (app_name, recipe) in &self.recipes {
            let app_dir = self
                .opts
                .dest
                .join(&self.opts.host_name)
                .join(&self.opts.timestamp)
                .join(app_name);

            if let Err(e) = fs::create_dir_all(&app_dir) {
                eprintln!("✗ {} backup failed: {}", app_name, e);
                results.push(RecipeOutcome {
                    app: app_name.clone(),
                    size_bytes: None,
                    error: Some(e.to_string()),
                });
                continue;
            }

            let mut progress = make_recipe_progress(app_name);
            let exec_result =
                executor.backup(recipe, &app_dir, &self.opts.parameters, &mut *progress);

            match exec_result {
                Ok(()) => {
                    let size = calculate_dir_size(&app_dir).unwrap_or(0);
                    if !output::is_verbose() {
                        output::success(&format!("{} ({})", app_name, output::format_size(size)));
                    }
                    results.push(RecipeOutcome {
                        app: app_name.clone(),
                        size_bytes: Some(size),
                        error: None,
                    });
                }
                Err(e) => {
                    let _ = fs::remove_dir_all(&app_dir);
                    eprintln!("✗ {} backup failed: {}", app_name, e);
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

#[cfg(not(test))]
fn make_recipe_progress(app: &str) -> Box<dyn Progress> {
    Box::new(TerminalProgress::new(&format!("Backing up {}", app)))
}

#[cfg(test)]
fn make_recipe_progress(app: &str) -> Box<dyn Progress> {
    Box::new(TerminalProgress::hidden(&format!("Backing up {}", app)))
}

pub fn restic_push(restic_repo: &str, restic_password: &str, backup_dir: &Path) -> Result<()> {
    output::info(&format!("Pushing {} to restic", backup_dir.display()));

    let mut progress = TerminalProgress::new("Checking restic repository");
    let snapshots_check = Command::new("restic")
        .arg("snapshots")
        .arg("--json")
        .env("RESTIC_REPOSITORY", restic_repo)
        .env("RESTIC_PASSWORD", restic_password)
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
            .env("RESTIC_REPOSITORY", restic_repo)
            .env("RESTIC_PASSWORD", restic_password)
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
            .env("RESTIC_REPOSITORY", restic_repo)
            .env("RESTIC_PASSWORD", restic_password)
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

pub fn restic_prune(restic_repo: &str, restic_password: &str, dry_run: bool) -> Result<()> {
    let mut progress = TerminalProgress::new("Pruning restic snapshots");

    let mut cmd = Command::new("restic");
    cmd.arg("forget")
        .arg("--keep-daily")
        .arg("7")
        .arg("--keep-weekly")
        .arg("4")
        .arg("--keep-monthly")
        .arg("12")
        .arg("--prune")
        .env("RESTIC_REPOSITORY", restic_repo)
        .env("RESTIC_PASSWORD", restic_password)
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
    use crate::playbook_meta::DbRecipe;
    use crate::services::backup::ssh::{MockSshSession, SshOp};

    fn baikal_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec![],
            paths: vec!["/opt/baikal/Specific".to_string()],
            owner: Some(("baikal".to_string(), "baikal".to_string())),
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
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
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
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
    fn create_runs_recipes_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let recipes = vec![
            ("baikal".to_string(), baikal_recipe()),
            ("bichon".to_string(), bichon_recipe()),
        ];
        let session = BackupSession::new(&mock, recipes, opts(tmp.path()));

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
        let session = BackupSession::new(&mock, recipes, opts(tmp.path()));

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
        mock.stage_run_result(crate::services::backup::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"connection refused".to_vec(),
        });

        let recipes = vec![
            ("paperless".to_string(), paperless_recipe()),
            ("baikal".to_string(), baikal_recipe()),
        ];
        let session = BackupSession::new(&mock, recipes, opts(tmp.path()));

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
        let session = BackupSession::new(&mock, vec![], opts(tmp.path()));

        let outcome = session.create().unwrap();

        assert!(outcome.results.is_empty());
        assert_eq!(outcome.timestamp, "2026-04-28_03-00-00");
        assert!(mock.calls().is_empty());
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
        };

        let mut session_params = HashMap::new();
        session_params.insert("include_music".to_string(), true);

        let opts_with_param = SessionOpts {
            host_name: "myserver".to_string(),
            dest: tmp.path().to_path_buf(),
            timestamp: "2026-04-28_03-00-00".to_string(),
            parameters: session_params,
        };

        let session = BackupSession::new(
            &mock,
            vec![("navidrome".to_string(), navidrome)],
            opts_with_param,
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
