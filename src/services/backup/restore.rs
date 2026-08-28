use crate::playbook_meta::BackupRecipe;
use crate::services::backup::executor::{RecipeExecutor, staged_parameters};
use crate::services::backup::session::{BackupSession, SessionOpts};
use crate::services::progress::Progress;
use crate::services::ssh::SshSession;
use eyre::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// One app's share of a restore: which app, where its staged backup is, and
/// the Recipe that decides what comes out of it.
pub struct RestoreTarget {
    pub app: String,
    pub backup_path: PathBuf,
    pub recipe: BackupRecipe,
}

#[derive(Debug, Clone)]
pub struct RestoreOpts {
    pub host_name: String,
    /// Where the emergency backup stages, as `{backup_root}/{host}/{timestamp}/`.
    pub backup_root: PathBuf,
    pub emergency_timestamp: String,
    /// A cross-host restore overwrites a Host other than the one the backup
    /// came from, which is what makes the emergency backup mandatory
    /// (ADR-0026); a same-host restore's rollback is the staged backup itself.
    pub cross_host: bool,
}

/// Which of the Session's steps a Progress is being built for.
///
/// The factory receives it so the command can label an emergency backup's bar
/// as a backup and a restore's as a restore — the render decision stays above
/// the seam (ADR-0047), and one factory serving both steps would otherwise
/// have to guess which it is serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestorePhase {
    EmergencyBackup,
    AppRestore,
}

/// A `Progress` per App and step, built by the caller (ADR-0047).
pub type RestoreProgressFactory<'a> = Box<dyn Fn(RestorePhase, &str) -> Box<dyn Progress> + 'a>;

/// What came of the redeploy the command injected.
///
/// A failed redeploy does not fail the restore — the bytes have landed, and
/// the ansible re-run exists to fix ownership the rsync could not set — so it
/// is an outcome the command renders, not an error the Session propagates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedeployOutcome {
    Completed,
    Failed(String),
    /// `--skip-playbook-unsafe`: the operator took the redeploy on themselves.
    SkippedUnsafe,
}

/// How ADR-0026's rollback guarantee was honored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyOutcome {
    /// Same-host restore: the staged backup being restored is the rollback.
    NotNeeded,
    Created {
        timestamp: String,
    },
    /// The backup failed and the injected decision chose to restore anyway.
    ContinuedWithout {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The emergency backup failed and the injected decision declined to
    /// continue; nothing was restored.
    Cancelled { emergency_error: String },
    Restored {
        emergency: EmergencyOutcome,
        redeploy: RedeployOutcome,
    },
}

/// Orchestrates one restore against one Host: emergency backup, then the
/// Recipe Executor per App, then redeploy, returning an outcome the command
/// renders.
///
/// The emergency backup runs inside `restore`, so no caller can reach the
/// Recipes without it. The redeploy and the continue-without-rollback decision
/// are injected: both end in the command layer (ansible machinery, a prompt),
/// and both are what tests fake to observe the orchestration.
pub struct RestoreSession<'a, S: SshSession + ?Sized> {
    ssh: &'a S,
    plan: &'a [RestoreTarget],
    opts: RestoreOpts,
    progress_for: RestoreProgressFactory<'a>,
    redeploy: Box<dyn FnOnce() -> RedeployOutcome + 'a>,
    continue_without_emergency: Box<dyn FnOnce(&str) -> bool + 'a>,
}

impl<'a, S: SshSession + ?Sized> RestoreSession<'a, S> {
    pub fn new(
        ssh: &'a S,
        plan: &'a [RestoreTarget],
        opts: RestoreOpts,
        progress_for: impl Fn(RestorePhase, &str) -> Box<dyn Progress> + 'a,
        redeploy: impl FnOnce() -> RedeployOutcome + 'a,
        continue_without_emergency: impl FnOnce(&str) -> bool + 'a,
    ) -> Self {
        Self {
            ssh,
            plan,
            opts,
            progress_for: Box::new(progress_for),
            redeploy: Box::new(redeploy),
            continue_without_emergency: Box::new(continue_without_emergency),
        }
    }

    /// An App failure aborts the run — later Apps are not attempted and the
    /// redeploy does not happen, unlike `BackupSession::create`, which carries
    /// on: a failed backup skips one App, a failed restore leaves the Host
    /// half-overwritten and every step after it would widen the damage.
    pub fn restore(self) -> Result<RestoreOutcome> {
        let Self {
            ssh,
            plan,
            opts,
            progress_for,
            redeploy,
            continue_without_emergency,
        } = self;

        let emergency = if opts.cross_host {
            match emergency_backup(ssh, plan, &opts, &progress_for) {
                Ok(()) => EmergencyOutcome::Created {
                    timestamp: opts.emergency_timestamp.clone(),
                },
                Err(e) => {
                    let error = format!("{e:#}");
                    if !continue_without_emergency(&error) {
                        return Ok(RestoreOutcome::Cancelled {
                            emergency_error: error,
                        });
                    }
                    EmergencyOutcome::ContinuedWithout { error }
                }
            }
        } else {
            EmergencyOutcome::NotNeeded
        };

        let executor = RecipeExecutor::new(ssh);
        for target in plan {
            let mut progress = progress_for(RestorePhase::AppRestore, &target.app);
            executor
                .restore(&target.recipe, &target.backup_path, &mut *progress)
                .wrap_err_with(|| format!("Failed to restore {}", target.app))?;
            progress.success(&format!("{} restore completed", target.app));
        }

        Ok(RestoreOutcome::Restored {
            emergency,
            redeploy: redeploy(),
        })
    }
}

/// Back up the target Host's current state before the restore overwrites it.
///
/// Created with the staged backup's own `staged_parameters`, not parameter
/// defaults: `rsync --delete` reaches every path the restore pushes, and a
/// rollback narrower than the blast radius is not a rollback (ADR-0026). Any
/// App failing its backup fails the whole emergency backup — a rollback
/// missing one App is not one either.
fn emergency_backup<S: SshSession + ?Sized>(
    ssh: &S,
    plan: &[RestoreTarget],
    opts: &RestoreOpts,
    progress_for: &RestoreProgressFactory<'_>,
) -> Result<()> {
    let mut parameters: HashMap<String, bool> = HashMap::new();
    for target in plan {
        for (name, present) in staged_parameters(&target.recipe, &target.backup_path) {
            *parameters.entry(name).or_insert(false) |= present;
        }
    }

    let recipes: Vec<(String, BackupRecipe)> = plan
        .iter()
        .map(|target| (target.app.clone(), target.recipe.clone()))
        .collect();

    let session = BackupSession::new(
        ssh,
        recipes,
        SessionOpts {
            host_name: opts.host_name.clone(),
            dest: opts.backup_root.clone(),
            timestamp: opts.emergency_timestamp.clone(),
            parameters,
        },
        |app| progress_for(RestorePhase::EmergencyBackup, app),
    );

    let outcome = session.create()?;
    let failed = outcome.failed_apps();
    if !failed.is_empty() {
        eyre::bail!("{} backup(s) failed", failed.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook_meta::{BackupParameter, DbEngine, DbRecipe};
    use crate::services::progress::{MockProgress, ProgressEvent};
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;

    fn recipe(paths: &[&str]) -> BackupRecipe {
        BackupRecipe {
            systemd_services: Vec::new(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
            attests: None,
            owner: None,
            db: None,
            post_restore_command: None,
            restore_advice: None,
            parameters: HashMap::new(),
        }
    }

    fn target(app: &str, recipe: BackupRecipe) -> RestoreTarget {
        RestoreTarget {
            app: app.to_string(),
            backup_path: PathBuf::from("/backups/vieille/2026-08-01_03-00-00").join(app),
            recipe,
        }
    }

    fn two_app_plan() -> Vec<RestoreTarget> {
        vec![
            target("baikal", recipe(&["/opt/baikal/Specific"])),
            target("bichon", recipe(&["/opt/bichon/data"])),
        ]
    }

    fn opts(backup_root: &Path, cross_host: bool) -> RestoreOpts {
        RestoreOpts {
            host_name: "myserver".to_string(),
            backup_root: backup_root.to_path_buf(),
            emergency_timestamp: "2026-08-28_09-00-00".to_string(),
            cross_host,
        }
    }

    /// A factory that records every step's events into one list, in the order
    /// the Session emitted them.
    fn recording(into: &MockProgress) -> impl Fn(RestorePhase, &str) -> Box<dyn Progress> + '_ {
        move |_phase, _app| Box::new(into.share())
    }

    fn rsync_to_remotes(calls: &[SshOp]) -> Vec<String> {
        calls
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncTo { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect()
    }

    fn rsync_from_remotes(calls: &[SshOp]) -> Vec<String> {
        calls
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncFrom { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn same_host_restore_pushes_each_app_and_takes_no_emergency_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let plan = two_app_plan();
        let events = MockProgress::new();

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), false),
            recording(&events),
            || RedeployOutcome::Completed,
            |_| panic!("a same-host restore has no emergency backup to fail"),
        );
        let outcome = session.restore().unwrap();

        assert_eq!(
            outcome,
            RestoreOutcome::Restored {
                emergency: EmergencyOutcome::NotNeeded,
                redeploy: RedeployOutcome::Completed,
            }
        );
        let calls = mock.calls();
        assert_eq!(
            rsync_to_remotes(&calls),
            vec!["/opt/baikal/Specific", "/opt/bichon/data"]
        );
        assert!(
            rsync_from_remotes(&calls).is_empty(),
            "a same-host restore must not back anything up first"
        );
    }

    #[test]
    fn cross_host_restore_backs_up_the_target_before_the_first_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let plan = two_app_plan();
        let events = MockProgress::new();

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), true),
            recording(&events),
            || RedeployOutcome::Completed,
            |_| panic!("the emergency backup succeeded, no decision to make"),
        );
        let outcome = session.restore().unwrap();

        assert_eq!(
            outcome,
            RestoreOutcome::Restored {
                emergency: EmergencyOutcome::Created {
                    timestamp: "2026-08-28_09-00-00".to_string()
                },
                redeploy: RedeployOutcome::Completed,
            }
        );

        let calls = mock.calls();
        let last_backup = calls
            .iter()
            .rposition(|c| matches!(c, SshOp::RsyncFrom { .. }))
            .expect("the emergency backup must pull the target's current state");
        let first_restore = calls
            .iter()
            .position(|c| matches!(c, SshOp::RsyncTo { .. }))
            .expect("the restore must push the staged backup");
        assert!(
            last_backup < first_restore,
            "every emergency backup op must precede the first restore op"
        );

        let staged = tmp
            .path()
            .join("myserver")
            .join("2026-08-28_09-00-00")
            .join("baikal");
        assert!(staged.is_dir(), "the emergency backup must stage locally");
    }

    // ADR-0026: the emergency backup's coverage must equal the restore's blast
    // radius, so its parameters are derived from the staged backup on disk.
    #[test]
    fn the_emergency_backup_covers_the_staged_backups_parameters() {
        let backup = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();

        let mut parameters = HashMap::new();
        parameters.insert(
            "include_music".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        let navidrome = BackupRecipe {
            systemd_services: vec!["navidrome".to_string()],
            paths: vec!["/var/lib/navidrome".to_string()],
            attests: None,
            owner: None,
            db: None,
            post_restore_command: None,
            restore_advice: None,
            parameters,
        };
        let staged = backup.path().join("navidrome");
        fs::create_dir_all(staged.join("srv/music")).unwrap();
        let plan = vec![RestoreTarget {
            app: "navidrome".to_string(),
            backup_path: staged,
            recipe: navidrome,
        }];

        let events = MockProgress::new();
        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(root.path(), true),
            recording(&events),
            || RedeployOutcome::Completed,
            |_| panic!("the emergency backup succeeded, no decision to make"),
        );
        session.restore().unwrap();

        assert!(
            rsync_from_remotes(&mock.calls()).contains(&"/srv/music".to_string()),
            "a music-bearing staged backup must put music in the rollback too"
        );
    }

    fn failing_emergency_plan() -> Vec<RestoreTarget> {
        let mut paperless = recipe(&["/opt/paperless/data"]);
        paperless.db = Some(DbRecipe {
            name: "paperless".to_string(),
            dump_path: "/tmp/paperless_db.dump".to_string(),
            engine: DbEngine::Postgres,
        });
        vec![target("paperless", paperless)]
    }

    fn failed_dump() -> CommandResult {
        CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"connection refused".to_vec(),
        }
    }

    #[test]
    fn a_declined_emergency_failure_cancels_before_any_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        mock.stage_run_result(failed_dump());
        let plan = failing_emergency_plan();
        let events = MockProgress::new();
        let asked: RefCell<Option<String>> = RefCell::new(None);

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), true),
            recording(&events),
            || panic!("a cancelled restore must not redeploy"),
            |error| {
                *asked.borrow_mut() = Some(error.to_string());
                false
            },
        );
        let outcome = session.restore().unwrap();

        assert_eq!(
            outcome,
            RestoreOutcome::Cancelled {
                emergency_error: "1 backup(s) failed".to_string()
            }
        );
        assert_eq!(asked.into_inner().unwrap(), "1 backup(s) failed");
        assert!(
            rsync_to_remotes(&mock.calls()).is_empty(),
            "declining the decision must leave the target untouched"
        );
    }

    #[test]
    fn an_accepted_emergency_failure_restores_anyway() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        mock.stage_run_result(failed_dump());
        let plan = failing_emergency_plan();
        let events = MockProgress::new();

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), true),
            recording(&events),
            || RedeployOutcome::Completed,
            |_| true,
        );
        let outcome = session.restore().unwrap();

        assert_eq!(
            outcome,
            RestoreOutcome::Restored {
                emergency: EmergencyOutcome::ContinuedWithout {
                    error: "1 backup(s) failed".to_string()
                },
                redeploy: RedeployOutcome::Completed,
            }
        );
        assert_eq!(rsync_to_remotes(&mock.calls()), vec!["/opt/paperless/data"]);
    }

    #[test]
    fn an_app_failure_aborts_the_run_before_later_apps_and_the_redeploy() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        // The first App's post_restore_command is the run's first `run` call.
        mock.stage_run_result(failed_dump());

        let mut broken = recipe(&["/opt/baikal/Specific"]);
        broken.post_restore_command = Some("false".to_string());
        let plan = vec![
            target("baikal", broken),
            target("bichon", recipe(&["/opt/bichon/data"])),
        ];
        let events = MockProgress::new();

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), false),
            recording(&events),
            || panic!("a failed restore must not redeploy"),
            |_| panic!("a same-host restore has no emergency backup to fail"),
        );
        let err = session.restore().unwrap_err().to_string();

        assert!(err.contains("Failed to restore baikal"), "{err}");
        assert!(
            !rsync_to_remotes(&mock.calls()).contains(&"/opt/bichon/data".to_string()),
            "an App after the failure must not be attempted"
        );
    }

    #[test]
    fn the_redeploy_runs_once_after_every_ssh_op_and_its_outcome_is_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let plan = two_app_plan();
        let events = MockProgress::new();
        let ops_seen_at_redeploy: RefCell<Option<usize>> = RefCell::new(None);

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), false),
            recording(&events),
            || {
                *ops_seen_at_redeploy.borrow_mut() = Some(mock.calls().len());
                RedeployOutcome::Failed("exit code: 2".to_string())
            },
            |_| panic!("a same-host restore has no emergency backup to fail"),
        );
        let outcome = session.restore().unwrap();

        assert_eq!(ops_seen_at_redeploy.into_inner(), Some(mock.calls().len()));
        assert_eq!(
            outcome,
            RestoreOutcome::Restored {
                emergency: EmergencyOutcome::NotNeeded,
                redeploy: RedeployOutcome::Failed("exit code: 2".to_string()),
            }
        );
    }

    #[test]
    fn each_step_asks_the_factory_by_phase_and_app() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let plan = two_app_plan();
        let asked: RefCell<Vec<(RestorePhase, String)>> = RefCell::new(Vec::new());

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), true),
            |phase, app| {
                asked.borrow_mut().push((phase, app.to_string()));
                Box::new(MockProgress::new())
            },
            || RedeployOutcome::Completed,
            |_| panic!("the emergency backup succeeded, no decision to make"),
        );
        session.restore().unwrap();

        assert_eq!(
            asked.into_inner(),
            vec![
                (RestorePhase::EmergencyBackup, "baikal".to_string()),
                (RestorePhase::EmergencyBackup, "bichon".to_string()),
                (RestorePhase::AppRestore, "baikal".to_string()),
                (RestorePhase::AppRestore, "bichon".to_string()),
            ]
        );
    }

    #[test]
    fn each_restored_app_is_reported_as_a_success_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockSshSession::new();
        let plan = two_app_plan();
        let events = MockProgress::new();

        let session = RestoreSession::new(
            &mock,
            &plan,
            opts(tmp.path(), false),
            recording(&events),
            || RedeployOutcome::Completed,
            |_| panic!("a same-host restore has no emergency backup to fail"),
        );
        session.restore().unwrap();

        let stream = events.events();
        assert!(stream.contains(&ProgressEvent::Success(
            "baikal restore completed".to_string()
        )));
        assert!(stream.contains(&ProgressEvent::Success(
            "bichon restore completed".to_string()
        )));
    }
}
