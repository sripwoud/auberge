use crate::playbook_meta::{BackupRecipe, DbEngine};
use crate::services::backup::deadman;
use crate::services::progress::Progress;
use crate::services::ssh::SshSession;
use eyre::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct RecipeExecutor<'a, S: SshSession + ?Sized> {
    session: &'a S,
}

impl<'a, S: SshSession + ?Sized> RecipeExecutor<'a, S> {
    pub fn new(session: &'a S) -> Self {
        Self { session }
    }

    pub fn backup(
        &self,
        app: &str,
        recipe: &BackupRecipe,
        dest_dir: &Path,
        parameters: &HashMap<String, bool>,
        progress: &mut dyn Progress,
    ) -> Result<()> {
        self.verify_path_attestation(recipe, parameters, progress)?;

        // A deadman only guards a quiesce window, so an App with nothing to
        // quiesce has nothing for it to protect (ADR-0066).
        let guarded = !recipe.systemd_services.is_empty();
        if guarded {
            let _ = deadman::check_and_report(self.session, app, progress);
            self.arm_deadman(app, recipe, progress);
        }

        let mut stopped: Vec<&str> = Vec::new();
        for service in &recipe.systemd_services {
            progress.task_started(&format!("Stopping {}", service));
            if let Err(e) = self.session.systemctl("stop", service) {
                self.restart_all(&stopped);
                if guarded {
                    deadman::disarm(self.session, app);
                }
                return Err(e);
            }
            stopped.push(service);
        }

        // Re-arm at the dump step boundary: what is bounded is how long any
        // one step may run, not the whole operation (ADR-0066).
        if guarded {
            self.arm_deadman(app, recipe, progress);
        }

        let result = (|| -> Result<()> {
            if let Some(db) = &recipe.db {
                let (tool, cmd) = match db.engine {
                    DbEngine::Postgres => (
                        "pg_dump",
                        format!(
                            "sudo -u postgres pg_dump -Fc -Z0 {} > {}",
                            db.name, db.dump_path
                        ),
                    ),
                    DbEngine::Mariadb => (
                        "mariadb-dump",
                        format!(
                            "sudo mariadb-dump --single-transaction {} > {}",
                            db.name, db.dump_path
                        ),
                    ),
                };
                progress.task_started(&format!("{tool} {}", db.name));
                let dump = self.session.run(&cmd)?;
                if !dump.success {
                    let _ = self.session.run(&format!("rm -f {}", db.dump_path));
                    eyre::bail!(
                        "{tool} failed for {}: {}",
                        db.name,
                        dump.stderr_str().trim()
                    );
                }
            }

            let paths = recipe.effective_paths(parameters);
            for path in &paths {
                // Re-armed per transfer, not once for the whole rsync phase:
                // the incident's own media rsync could plausibly run for tens
                // of minutes on its own (ADR-0066).
                if guarded {
                    self.arm_deadman(app, recipe, progress);
                }
                progress.task_started(&format!("rsync {}", path));
                self.session.rsync_from(path, dest_dir)?;
            }

            if let Some(db) = &recipe.db {
                progress.task_started("Fetching database dump");
                let local_dump = dest_dir.join("db.dump");
                self.session.scp_from(&db.dump_path, &local_dump)?;
                let _ = self.session.run(&format!("rm -f {}", db.dump_path));
            }

            Ok(())
        })();

        // Re-arm at the restart step boundary, then disarm once the units are
        // back up — every exit path from here, success or failure, leaves the
        // App up again, so nothing stays armed once it is (ADR-0066).
        if guarded {
            self.arm_deadman(app, recipe, progress);
        }
        for service in &stopped {
            progress.task_started(&format!("Starting {}", service));
        }
        let restart_failures = self.restart_all_collecting(&stopped);
        if guarded {
            deadman::disarm(self.session, app);
        }
        progress.task_done();

        match result {
            Ok(()) if restart_failures.is_empty() => Ok(()),
            Ok(()) => eyre::bail!(
                "Backup succeeded but failed to restart services:\n  {}",
                restart_failures.join("\n  ")
            ),
            Err(e) if restart_failures.is_empty() => Err(e),
            Err(e) => eyre::bail!(
                "Backup failed: {e}\nAdditionally, failed to restart services:\n  {}",
                restart_failures.join("\n  ")
            ),
        }
    }

    /// Check the Recipe's declared paths against the App's Path Attestation,
    /// before anything is stopped or staged (ADR-0033).
    ///
    /// Runs first for two reasons: a mismatch then costs no service bounce,
    /// and no snapshot exists to be restored from later by someone for whom
    /// only the exit code has been forgotten.
    ///
    /// A query that reports nothing is not a failure. Every way of failing to
    /// *ask* — a table an upstream release renamed, a mistyped invocation, the
    /// wrong database — exits non-zero and is caught above; reaching here with
    /// no rows means the App genuinely holds no data yet. That is the normal
    /// state of a freshly deployed App, including the restore target of a
    /// cross-host migration, whose emergency backup runs through here first.
    fn verify_path_attestation(
        &self,
        recipe: &BackupRecipe,
        parameters: &HashMap<String, bool>,
        progress: &mut dyn Progress,
    ) -> Result<()> {
        let Some(query) = &recipe.attests else {
            return Ok(());
        };
        progress.task_started("Verifying declared paths");

        let answer = self.session.run(query)?;
        if !answer.success {
            eyre::bail!(
                "the coverage query failed, so the declared paths are unverified: {}",
                answer.stderr_str().trim()
            );
        }

        let reported: Vec<String> = answer
            .stdout_str()
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();

        let declared = recipe.effective_paths(parameters);
        let uncovered: Vec<&str> = reported
            .iter()
            .map(String::as_str)
            .filter(|path| !is_within(path, &declared))
            .collect();
        if !uncovered.is_empty() {
            eyre::bail!(
                "the App holds data no declared path covers, so a snapshot taken now would \
                 restore metadata pointing at nothing.\n  uncovered: {}\n  declared:  {}\n\
                 Add the path to `paths:` in the Playbook Meta, or point the App back.",
                uncovered.join(", "),
                declared.join(", ")
            );
        }
        Ok(())
    }

    /// Restore whatever the staged backup holds — see [`staged_paths`] for
    /// why this takes no parameter map.
    pub fn restore(
        &self,
        recipe: &BackupRecipe,
        source_dir: &Path,
        progress: &mut dyn Progress,
    ) -> Result<()> {
        let mut stopped: Vec<&str> = Vec::new();
        for service in &recipe.systemd_services {
            progress.task_started(&format!("Stopping {}", service));
            if let Err(e) = self.session.systemctl("stop", service) {
                self.restart_all(&stopped);
                return Err(e);
            }
            stopped.push(service);
        }

        let result = (|| -> Result<()> {
            let paths = staged_paths(recipe, source_dir);
            for path in &paths {
                progress.task_started(&format!("rsync {}", path));
                self.session
                    .rsync_to(&staged_copy(source_dir, path), path)?;
            }

            if let Some((user, group)) = &recipe.owner {
                for path in &paths {
                    progress.task_started(&format!("chown {}", path));
                    self.session.set_ownership(path, user, group)?;
                }
            }

            if let Some(db) = &recipe.db {
                let local_dump = source_dir.join("db.dump");
                if local_dump.exists() {
                    let (tool, cmd) = match db.engine {
                        DbEngine::Postgres => (
                            "pg_restore",
                            format!(
                                "sudo -u postgres pg_restore --clean --if-exists -d {} {} 2>&1",
                                db.name, db.dump_path
                            ),
                        ),
                        DbEngine::Mariadb => (
                            "mariadb",
                            format!("sudo mariadb {} < {} 2>&1", db.name, db.dump_path),
                        ),
                    };
                    progress.task_started(&format!("{tool} {}", db.name));
                    self.session.scp_to(&local_dump, &db.dump_path)?;
                    self.session.run(&format!("chmod 644 {}", db.dump_path))?;
                    let restore = self.session.run(&cmd)?;
                    let _ = self.session.run(&format!("rm -f {}", db.dump_path));
                    let warnings_only = db.engine == DbEngine::Postgres
                        && pg_restore_warnings_only(&restore.stdout_str());
                    if !restore.success && !warnings_only {
                        eyre::bail!("{tool} failed: {}", restore.stdout_str().trim());
                    }
                }
            }

            if let Some(cmd) = &recipe.post_restore_command {
                progress.task_started("Running post_restore_command");
                let post = self.session.run(cmd)?;
                if !post.success {
                    eyre::bail!("post_restore_command failed: {}", post.stderr_str().trim());
                }
            }

            Ok(())
        })();

        for service in &stopped {
            progress.task_started(&format!("Starting {}", service));
        }
        let restart_failures = self.restart_all_collecting(&stopped);
        progress.task_done();

        match result {
            Ok(()) if restart_failures.is_empty() => Ok(()),
            Ok(()) => eyre::bail!(
                "Restore succeeded but failed to restart services:\n  {}",
                restart_failures.join("\n  ")
            ),
            Err(e) if restart_failures.is_empty() => Err(e),
            Err(e) => eyre::bail!(
                "Restore failed: {e}\nAdditionally, failed to restart services:\n  {}",
                restart_failures.join("\n  ")
            ),
        }
    }

    /// Arms `app`'s deadman, warning through `progress` if it fails.
    ///
    /// A silent failure here would defeat the whole mechanism: the window it
    /// was meant to guard runs unprotected with nothing telling the operator
    /// so. The `Result` still is not propagated with `?` — an arm failure
    /// must never skip the guaranteed restart that follows it, only leave a
    /// visible trace that this window had no Host-side backstop.
    fn arm_deadman(&self, app: &str, recipe: &BackupRecipe, progress: &mut dyn Progress) {
        if let Err(e) = deadman::arm(self.session, app, &recipe.systemd_services) {
            progress.warn(&format!(
                "{app}: failed to arm the backup deadman ({e}); this window has no Host-side \
                 recovery if the driver dies before finishing it"
            ));
        }
    }

    fn restart_all(&self, services: &[&str]) {
        for service in services {
            let _ = self.session.systemctl("start", service);
        }
    }

    fn restart_all_collecting(&self, services: &[&str]) -> Vec<String> {
        services
            .iter()
            .filter_map(|s| {
                self.session
                    .systemctl("start", s)
                    .err()
                    .map(|e| format!("{}: {}", s, e))
            })
            .collect()
    }
}

/// Whether a declared path carries `path` into the backup.
///
/// `rsync` of a directory carries everything beneath it, so containment counts
/// — but only at a path boundary: `/srv/books` does not hold
/// `/srv/books-archive`.
fn is_within(path: &str, declared: &[String]) -> bool {
    declared.iter().any(|root| {
        let root = root.trim_end_matches('/');
        root == path || path.starts_with(&format!("{root}/"))
    })
}

/// The paths a staged backup holds: the Recipe's declared paths, plus every
/// parameter-gated path present on disk. A Recipe's `parameters` are a
/// create-time input — the choice is recorded nowhere a later restore can
/// read it, so resolving the Recipe against parameter defaults dropped
/// 19.92 GB of music from every navidrome restore while reporting success
/// (ADR-0026). Optional paths are appended sorted, so the plan an operator
/// reads and the order pushed are stable across `HashMap` iteration.
pub fn staged_paths(recipe: &BackupRecipe, source_dir: &Path) -> Vec<String> {
    let mut optional: Vec<String> = recipe
        .parameters
        .values()
        .flat_map(|parameter| parameter.adds_paths.iter())
        .filter(|path| staged_copy(source_dir, path).exists())
        .cloned()
        .collect();
    optional.sort();

    let mut paths = recipe.paths.clone();
    paths.extend(optional);
    paths
}

/// The parameter values a staged backup implies, on when any path the
/// parameter adds is present. What a `backup create` needs to cover the paths
/// a restore from this backup will overwrite: `rsync --delete` reaches every
/// path [`staged_paths`] returns, so a pre-migration backup taken with less
/// than this is not a rollback (ADR-0026).
pub fn staged_parameters(recipe: &BackupRecipe, source_dir: &Path) -> HashMap<String, bool> {
    recipe
        .parameters
        .iter()
        .map(|(name, parameter)| {
            let present = parameter
                .adds_paths
                .iter()
                .any(|path| staged_copy(source_dir, path).exists());
            (name.clone(), present)
        })
        .collect()
}

/// Where a staged backup keeps its copy of a remote path: `rsync --relative`
/// preserves the tree, so `/srv/music` lands at `<app>/srv/music`.
fn staged_copy(source_dir: &Path, path: &str) -> PathBuf {
    source_dir.join(path.trim_start_matches('/'))
}

fn pg_restore_warnings_only(text: &str) -> bool {
    text.lines().all(|line| {
        let trimmed = line.trim().to_lowercase();
        trimmed.is_empty()
            || trimmed.contains("warning")
            || trimmed.starts_with("pg_restore: warning")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook_meta::{BackupParameter, DbEngine, DbRecipe};
    use crate::services::ssh::{MockSshSession, SshOp};

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

    /// The bichon Recipe as shipped, not a hand-built stand-in: what #619 is
    /// about is the order of the units the repo actually declares.
    fn shipped_bichon_recipe() -> BackupRecipe {
        let playbooks_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("playbooks");
        crate::services::backup::recipe::load_app_recipe(&playbooks_dir, "bichon", "alice").unwrap()
    }

    /// Strips every SSH call a deadman (ADR-0066) issues to guard the quiesce
    /// window, leaving the App's own stop/dump/rsync/start sequence — so a
    /// test asserting that sequence by position does not have to know how
    /// many arm/disarm/fire-check calls a guarded recipe adds around it.
    fn without_deadman_ops(calls: Vec<SshOp>) -> Vec<SshOp> {
        calls
            .into_iter()
            .filter(|c| match c {
                SshOp::RunDetached(_) => false,
                SshOp::Run(cmd) => !cmd.contains("deadman"),
                _ => true,
            })
            .collect()
    }

    fn systemctl_sequence(calls: &[SshOp]) -> Vec<(String, String)> {
        calls
            .iter()
            .filter_map(|c| match c {
                SshOp::Systemctl { action, service } => Some((action.clone(), service.clone())),
                _ => None,
            })
            .collect()
    }

    /// The archive timer is quiesced ahead of the server it triggers, so an
    /// hourly tick landing mid-rsync cannot pull bichon back up through
    /// `Requires=bichon.service` while its store is being copied (#619).
    #[test]
    fn test_backup_bichon_stops_the_archive_timer_before_the_server() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "bichon",
                &shipped_bichon_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        assert_eq!(
            systemctl_sequence(&mock.calls()),
            vec![
                ("stop".to_string(), "bichon-archive.timer".to_string()),
                ("stop".to_string(), "bichon".to_string()),
                ("start".to_string(), "bichon-archive.timer".to_string()),
                ("start".to_string(), "bichon".to_string()),
            ]
        );
    }

    /// Restore quiesces the same pair: a tick mid-rsync would let the server
    /// write into a half-restored store.
    #[test]
    fn test_restore_bichon_stops_the_archive_timer_before_the_server() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        let source = tempfile::tempdir().unwrap();
        executor
            .restore(&shipped_bichon_recipe(), source.path(), &mut progress)
            .unwrap();

        assert_eq!(
            systemctl_sequence(&mock.calls()),
            vec![
                ("stop".to_string(), "bichon-archive.timer".to_string()),
                ("stop".to_string(), "bichon".to_string()),
                ("start".to_string(), "bichon-archive.timer".to_string()),
                ("start".to_string(), "bichon".to_string()),
            ]
        );
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
            post_restore_command: Some("sudo -u paperless ./manage.py migrate".to_string()),
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        }
    }

    fn grimmory_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec!["grimmory".to_string()],
            paths: vec!["/srv/grimmory".to_string()],
            owner: Some(("grimmory".to_string(), "grimmory".to_string())),
            db: Some(DbRecipe {
                name: "grimmory".to_string(),
                dump_path: "/tmp/grimmory_db.dump".to_string(),
                engine: DbEngine::Mariadb,
            }),
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        }
    }

    /// What grimmory's Playbook Meta asks: the library roots the App itself
    /// records, one per line.
    const LIBRARY_QUERY: &str = "sudo mariadb -N -B -e 'select path from library_path' grimmory";

    /// A Recipe whose declared paths are checked against what the App attests.
    fn attesting_recipe(paths: &[&str]) -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec!["grimmory".to_string()],
            paths: paths.iter().map(|path| path.to_string()).collect(),
            owner: None,
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: Some(LIBRARY_QUERY.to_string()),
            restore_advice: None,
        }
    }

    /// Stage the App's attestation.
    fn reports(mock: &MockSshSession, stdout: &str) {
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        });
    }

    fn backup_with(recipe: &BackupRecipe, mock: &MockSshSession) -> Result<()> {
        let executor = RecipeExecutor::new(mock);
        let mut progress = crate::services::progress::MockProgress::new();
        let tmp = tempfile::tempdir().unwrap();
        executor.backup(
            "grimmory",
            recipe,
            tmp.path(),
            &HashMap::new(),
            &mut progress,
        )
    }

    fn navidrome_recipe() -> BackupRecipe {
        let mut params = HashMap::new();
        params.insert(
            "include_music".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        BackupRecipe {
            systemd_services: vec!["navidrome".to_string()],
            paths: vec!["/var/lib/navidrome".to_string()],
            owner: Some(("navidrome".to_string(), "navidrome".to_string())),
            db: None,
            post_restore_command: None,
            parameters: params,
            attests: None,
            restore_advice: None,
        }
    }

    fn syncthing_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec!["syncthing@alice".to_string()],
            paths: vec![
                "/home/alice/.local/state/syncthing/config.xml".to_string(),
                "/home/alice/.local/state/syncthing/cert.pem".to_string(),
                "/home/alice/.local/state/syncthing/key.pem".to_string(),
            ],
            owner: Some(("alice".to_string(), "alice".to_string())),
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        }
    }

    #[test]
    fn test_backup_syncthing_stops_unit_rsyncs_identity_files_then_starts() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "syncthing",
                &syncthing_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let calls = without_deadman_ops(mock.calls());
        assert_eq!(
            calls[0],
            SshOp::Systemctl {
                action: "stop".to_string(),
                service: "syncthing@alice".to_string(),
            }
        );
        let rsync_remotes: Vec<String> = calls
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncFrom { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            rsync_remotes,
            vec![
                "/home/alice/.local/state/syncthing/config.xml",
                "/home/alice/.local/state/syncthing/cert.pem",
                "/home/alice/.local/state/syncthing/key.pem",
            ]
        );
        assert_eq!(
            calls.last().unwrap(),
            &SshOp::Systemctl {
                action: "start".to_string(),
                service: "syncthing@alice".to_string(),
            }
        );
    }

    #[test]
    fn test_restore_syncthing_rsyncs_identity_files_and_chowns_to_admin_user() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&syncthing_recipe(), Path::new("/tmp/source"), &mut progress)
            .unwrap();

        let calls = mock.calls();
        assert!(matches!(
            &calls[1],
            SshOp::RsyncTo { local, remote }
            if remote == "/home/alice/.local/state/syncthing/config.xml"
                && local.ends_with("home/alice/.local/state/syncthing/config.xml")
        ));
        assert!(calls.contains(&SshOp::SetOwnership {
            remote: "/home/alice/.local/state/syncthing/key.pem".to_string(),
            user: "alice".to_string(),
            group: "alice".to_string(),
        }));
    }

    #[test]
    fn test_backup_no_services_just_rsyncs_paths() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "baikal",
                &baikal_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            &calls[0],
            SshOp::RsyncFrom { remote, .. } if remote == "/opt/baikal/Specific"
        ));
    }

    #[test]
    fn test_backup_stops_then_rsyncs_then_starts() {
        let mock = MockSshSession::new();
        let recipe = BackupRecipe {
            systemd_services: vec!["bichon".to_string()],
            paths: vec!["/opt/bichon/data".to_string()],
            owner: None,
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        };
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "bichon",
                &recipe,
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let calls = without_deadman_ops(mock.calls());
        assert_eq!(
            calls[0],
            SshOp::Systemctl {
                action: "stop".to_string(),
                service: "bichon".to_string(),
            }
        );
        assert!(matches!(&calls[1], SshOp::RsyncFrom { .. }));
        assert_eq!(
            calls[2],
            SshOp::Systemctl {
                action: "start".to_string(),
                service: "bichon".to_string(),
            }
        );
    }

    #[test]
    fn test_backup_with_db_runs_pg_dump_before_rsync_then_scps_dump() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "paperless",
                &paperless_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let calls = without_deadman_ops(mock.calls());
        assert_eq!(
            calls[0],
            SshOp::Systemctl {
                action: "stop".to_string(),
                service: "paperless-webserver".to_string(),
            }
        );
        match &calls[1] {
            SshOp::Run(cmd) => {
                assert!(cmd.contains("pg_dump -Fc -Z0"));
                assert!(cmd.contains("paperless"));
                assert!(cmd.contains("/tmp/paperless_db.dump"));
            }
            other => panic!("expected pg_dump Run, got {other:?}"),
        }
        assert!(matches!(&calls[2], SshOp::RsyncFrom { .. }));
        assert!(matches!(
            &calls[3],
            SshOp::ScpFrom { remote, .. } if remote == "/tmp/paperless_db.dump"
        ));
        match &calls[4] {
            SshOp::Run(cmd) => assert!(cmd.contains("rm -f /tmp/paperless_db.dump")),
            other => panic!("expected rm -f Run, got {other:?}"),
        }
        assert_eq!(
            calls[5],
            SshOp::Systemctl {
                action: "start".to_string(),
                service: "paperless-webserver".to_string(),
            }
        );
    }

    #[test]
    fn test_backup_with_mariadb_db_runs_mariadb_dump() {
        use crate::services::progress::ProgressEvent;
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "grimmory",
                &grimmory_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let calls = without_deadman_ops(mock.calls());
        match &calls[1] {
            SshOp::Run(cmd) => assert_eq!(
                cmd,
                "sudo mariadb-dump --single-transaction grimmory > /tmp/grimmory_db.dump"
            ),
            other => panic!("expected mariadb-dump Run, got {other:?}"),
        }
        assert!(matches!(
            &calls[3],
            SshOp::ScpFrom { remote, .. } if remote == "/tmp/grimmory_db.dump"
        ));
        assert!(progress.events().iter().any(|e| matches!(
            e,
            ProgressEvent::TaskStarted(s) if s == "mariadb-dump grimmory"
        )));
    }

    #[test]
    fn test_restore_with_mariadb_db_pipes_dump_into_mariadb() {
        use crate::services::progress::ProgressEvent;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("db.dump"), b"-- dump").unwrap();

        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&grimmory_recipe(), tmp.path(), &mut progress)
            .unwrap();

        let calls = mock.calls();
        assert!(calls.iter().any(|c| matches!(
            c,
            SshOp::Run(cmd) if cmd == "sudo mariadb grimmory < /tmp/grimmory_db.dump 2>&1"
        )));
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, SshOp::Run(cmd) if cmd.contains("pg_restore"))),
            "mariadb recipe must not fall through to pg_restore"
        );
        assert!(progress.events().iter().any(|e| matches!(
            e,
            ProgressEvent::TaskStarted(s) if s == "mariadb grimmory"
        )));
    }

    #[test]
    fn test_mariadb_restore_failure_is_not_excused_as_warnings_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("db.dump"), b"-- dump").unwrap();

        let mock = MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::ok());
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: b"Warning: something looked warning-shaped but the exit was fatal".to_vec(),
            stderr: Vec::new(),
        });
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        let result = executor.restore(&grimmory_recipe(), tmp.path(), &mut progress);

        assert!(
            result.is_err(),
            "warnings-only leniency is pg_restore-specific; mariadb exit 1 must fail"
        );
    }

    #[test]
    fn test_backup_with_include_music_parameter_adds_path() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut params = HashMap::new();
        params.insert("include_music".to_string(), true);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "navidrome",
                &navidrome_recipe(),
                Path::new("/tmp/dest"),
                &params,
                &mut progress,
            )
            .unwrap();

        let rsync_remotes: Vec<String> = mock
            .calls()
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncFrom { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect();
        assert!(rsync_remotes.contains(&"/var/lib/navidrome".to_string()));
        assert!(rsync_remotes.contains(&"/srv/music".to_string()));
    }

    #[test]
    fn test_backup_omits_optional_path_when_parameter_absent() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "navidrome",
                &navidrome_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let rsync_remotes: Vec<String> = mock
            .calls()
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncFrom { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect();
        assert!(rsync_remotes.contains(&"/var/lib/navidrome".to_string()));
        assert!(!rsync_remotes.contains(&"/srv/music".to_string()));
    }

    #[test]
    fn test_restore_rsyncs_then_sets_ownership_then_starts_services() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let recipe = BackupRecipe {
            systemd_services: vec!["freshrss".to_string()],
            paths: vec!["/var/lib/freshrss".to_string()],
            owner: Some(("freshrss".to_string(), "freshrss".to_string())),
            db: None,
            post_restore_command: None,
            parameters: HashMap::new(),
            attests: None,
            restore_advice: None,
        };
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&recipe, Path::new("/tmp/source"), &mut progress)
            .unwrap();

        let calls = mock.calls();
        assert_eq!(
            calls[0],
            SshOp::Systemctl {
                action: "stop".to_string(),
                service: "freshrss".to_string(),
            }
        );
        assert!(matches!(
            &calls[1],
            SshOp::RsyncTo { remote, .. } if remote == "/var/lib/freshrss"
        ));
        assert_eq!(
            calls[2],
            SshOp::SetOwnership {
                remote: "/var/lib/freshrss".to_string(),
                user: "freshrss".to_string(),
                group: "freshrss".to_string(),
            }
        );
        assert_eq!(
            calls[3],
            SshOp::Systemctl {
                action: "start".to_string(),
                service: "freshrss".to_string(),
            }
        );
    }

    #[test]
    fn test_restore_runs_post_restore_command_after_db_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = tmp.path().join("db.dump");
        std::fs::write(&dump, b"binary").unwrap();

        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&paperless_recipe(), tmp.path(), &mut progress)
            .unwrap();

        let calls = mock.calls();
        let scp_to_idx = calls
            .iter()
            .position(
                |c| matches!(c, SshOp::ScpTo { remote, .. } if remote == "/tmp/paperless_db.dump"),
            )
            .expect("should scp dump to remote");
        let pg_restore_idx = calls
            .iter()
            .position(|c| matches!(c, SshOp::Run(cmd) if cmd.contains("pg_restore")))
            .expect("should pg_restore");
        let post_idx = calls
            .iter()
            .position(|c| matches!(c, SshOp::Run(cmd) if cmd.contains("manage.py migrate")))
            .expect("should run post_restore_command");
        assert!(scp_to_idx < pg_restore_idx);
        assert!(pg_restore_idx < post_idx);
    }

    #[test]
    fn test_restore_skips_db_when_local_dump_missing() {
        let tmp = tempfile::tempdir().unwrap();

        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&paperless_recipe(), tmp.path(), &mut progress)
            .unwrap();

        let calls = mock.calls();
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, SshOp::Run(cmd) if cmd.contains("pg_restore"))),
            "should not run pg_restore when dump file missing"
        );
        assert!(
            !calls.iter().any(
                |c| matches!(c, SshOp::ScpTo { remote, .. } if remote == "/tmp/paperless_db.dump")
            ),
            "should not scp dump when missing"
        );
    }

    #[test]
    fn backup_emits_task_started_per_step_via_progress() {
        use crate::services::progress::ProgressEvent;
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "paperless",
                &paperless_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let names: Vec<String> = progress
            .events()
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::TaskStarted(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert!(names.iter().any(|n| n == "Stopping paperless-webserver"));
        assert!(names.iter().any(|n| n == "pg_dump paperless"));
        assert!(names.iter().any(|n| n.starts_with("rsync ")));
        assert!(names.iter().any(|n| n == "Fetching database dump"));
        assert!(names.iter().any(|n| n == "Starting paperless-webserver"));
        assert!(matches!(
            progress.events().last(),
            Some(ProgressEvent::TaskDone)
        ));
    }

    fn staged_backup(paths: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for path in paths {
            std::fs::create_dir_all(tmp.path().join(path.trim_start_matches('/'))).unwrap();
        }
        tmp
    }

    #[test]
    fn test_restore_includes_optional_path_the_backup_holds() {
        let backup = staged_backup(&["/var/lib/navidrome", "/srv/music"]);
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&navidrome_recipe(), backup.path(), &mut progress)
            .unwrap();

        let calls = mock.calls();
        let rsync_remotes: Vec<String> = calls
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncTo { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(rsync_remotes, vec!["/var/lib/navidrome", "/srv/music"]);
        assert!(calls.contains(&SshOp::SetOwnership {
            remote: "/srv/music".to_string(),
            user: "navidrome".to_string(),
            group: "navidrome".to_string(),
        }));
    }

    #[test]
    fn test_restore_omits_optional_path_the_backup_lacks() {
        let backup = staged_backup(&["/var/lib/navidrome"]);
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .restore(&navidrome_recipe(), backup.path(), &mut progress)
            .unwrap();

        let rsync_remotes: Vec<String> = mock
            .calls()
            .iter()
            .filter_map(|c| match c {
                SshOp::RsyncTo { remote, .. } => Some(remote.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(rsync_remotes, vec!["/var/lib/navidrome"]);
    }

    #[test]
    fn test_staged_paths_appends_present_optional_paths_sorted() {
        let mut params = HashMap::new();
        params.insert(
            "include_music".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        params.insert(
            "include_artwork".to_string(),
            BackupParameter {
                default: false,
                adds_paths: vec!["/srv/artwork".to_string()],
            },
        );
        let recipe = BackupRecipe {
            parameters: params,
            attests: None,
            ..navidrome_recipe()
        };
        let backup = staged_backup(&["/srv/music", "/srv/artwork"]);

        assert_eq!(
            staged_paths(&recipe, backup.path()),
            vec!["/var/lib/navidrome", "/srv/artwork", "/srv/music"]
        );
    }

    #[test]
    fn test_staged_paths_ignores_a_parameter_default_of_true() {
        let mut params = HashMap::new();
        params.insert(
            "include_music".to_string(),
            BackupParameter {
                default: true,
                adds_paths: vec!["/srv/music".to_string()],
            },
        );
        let recipe = BackupRecipe {
            parameters: params,
            attests: None,
            ..navidrome_recipe()
        };
        let backup = staged_backup(&["/var/lib/navidrome"]);

        assert_eq!(
            staged_paths(&recipe, backup.path()),
            vec!["/var/lib/navidrome"]
        );
    }

    #[test]
    fn test_staged_paths_keeps_declared_paths_the_backup_lacks() {
        let backup = staged_backup(&[]);
        assert_eq!(
            staged_paths(&baikal_recipe(), backup.path()),
            vec!["/opt/baikal/Specific"]
        );
    }

    #[test]
    fn test_staged_parameters_are_on_when_the_backup_holds_their_path() {
        let backup = staged_backup(&["/var/lib/navidrome", "/srv/music"]);
        assert_eq!(
            staged_parameters(&navidrome_recipe(), backup.path()),
            HashMap::from([("include_music".to_string(), true)])
        );
    }

    #[test]
    fn test_staged_parameters_are_off_when_the_backup_lacks_their_path() {
        let backup = staged_backup(&["/var/lib/navidrome"]);
        assert_eq!(
            staged_parameters(&navidrome_recipe(), backup.path()),
            HashMap::from([("include_music".to_string(), false)])
        );
    }

    #[test]
    fn test_staged_parameters_is_empty_for_a_recipe_without_parameters() {
        let backup = staged_backup(&["/opt/baikal/Specific"]);
        assert!(staged_parameters(&baikal_recipe(), backup.path()).is_empty());
    }

    #[test]
    fn test_backup_failed_pg_dump_still_restarts_services() {
        let mock = MockSshSession::new();
        // The deadman's fire-check is the first `run()` call for a guarded
        // recipe; stage its "no marker" answer before pg_dump's failure so
        // the second staged result is the one pg_dump actually consumes.
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
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        let result = executor.backup(
            "paperless",
            &paperless_recipe(),
            Path::new("/tmp/dest"),
            &HashMap::new(),
            &mut progress,
        );
        assert!(result.is_err());

        let starts: Vec<_> = mock
            .calls()
            .iter()
            .filter(|c| matches!(c, SshOp::Systemctl { action, .. } if action == "start"))
            .cloned()
            .collect();
        assert!(
            !starts.is_empty(),
            "services must be restarted even when pg_dump fails"
        );
    }

    #[test]
    fn test_the_attestation_is_checked_before_anything_is_stopped() {
        let mock = MockSshSession::new();
        reports(&mock, "/srv/books\n");

        let result = backup_with(&attesting_recipe(&["/srv/grimmory"]), &mock);

        assert!(result.is_err(), "an uncovered library must fail the backup");
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(LIBRARY_QUERY.to_string())],
            "the check runs before the quiesce, so a mismatch costs no service bounce and \
             stages no snapshot anyone could later restore from"
        );
    }

    #[test]
    fn test_the_failure_names_the_path_that_is_not_covered() {
        let mock = MockSshSession::new();
        reports(&mock, "/srv/grimmory\n/srv/books\n");

        let error = backup_with(&attesting_recipe(&["/srv/grimmory"]), &mock)
            .expect_err("an uncovered path must fail");

        let message = format!("{error:#}");
        assert!(
            message.contains("/srv/books"),
            "the operator has to know which path to declare: {message}"
        );
    }

    #[test]
    fn test_a_reported_path_nested_under_a_declared_one_is_covered() {
        let mock = MockSshSession::new();
        reports(&mock, "/srv/books\n");

        backup_with(&attesting_recipe(&["/srv"]), &mock)
            .expect("rsync of a parent carries everything under it");
    }

    #[test]
    fn test_a_shared_prefix_that_is_not_a_parent_does_not_hold_the_path() {
        let mock = MockSshSession::new();
        reports(&mock, "/srv/books-archive\n");

        assert!(
            backup_with(&attesting_recipe(&["/srv/books"]), &mock).is_err(),
            "/srv/books does not contain /srv/books-archive; a textual prefix is not a path \
             boundary"
        );
    }

    #[test]
    fn test_an_app_holding_no_data_yet_attests_nothing_and_still_backs_up() {
        let mock = MockSshSession::new();
        reports(&mock, "\n  \n");

        backup_with(&attesting_recipe(&["/srv/books"]), &mock).expect(
            "a freshly deployed App reports no data locations, and the emergency backup a \
             cross-host restore takes of its target runs through here before the migration \
             that fills it",
        );
    }

    #[test]
    fn test_an_attestation_that_fails_fails_the_backup() {
        let mock = MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"ERROR 1146: Table 'grimmory.library_path' doesn't exist".to_vec(),
        });

        let error = backup_with(&attesting_recipe(&["/srv/books"]), &mock)
            .expect_err("an attestation that errored proves nothing");

        assert!(
            format!("{error:#}").contains("library_path"),
            "the exit code is what separates failing to ask from having nothing to say, so \
             the query's own stderr is the only clue to why it could not answer"
        );
    }

    #[test]
    fn test_a_recipe_without_an_attestation_is_asked_nothing() {
        let mock = MockSshSession::new();

        backup_with(&baikal_recipe(), &mock).expect("baikal declares no coverage query");

        assert!(
            !mock
                .calls()
                .iter()
                .any(|call| matches!(call, SshOp::Run(cmd) if cmd.contains("select"))),
            "the check is opt-in per Recipe; every App without `attests:` is untouched"
        );
    }

    // ADR-0066: a Host-side deadman guards the quiesce window.

    fn detached_calls(calls: &[SshOp]) -> Vec<String> {
        calls
            .iter()
            .filter_map(|c| match c {
                SshOp::RunDetached(cmd) => Some(cmd.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn backup_arms_a_deadman_before_quiescing_a_guarded_recipe() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "paperless",
                &paperless_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let calls = mock.calls();
        let first_arm = calls
            .iter()
            .position(|c| matches!(c, SshOp::RunDetached(_)))
            .expect("a guarded recipe must arm a deadman");
        let first_stop = calls
            .iter()
            .position(|c| matches!(c, SshOp::Systemctl { action, .. } if action == "stop"))
            .unwrap();
        assert!(
            first_arm < first_stop,
            "the deadman must be armed before anything is quiesced"
        );
    }

    #[test]
    fn backup_disarms_the_deadman_as_the_very_last_call_on_a_clean_finish() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "paperless",
                &paperless_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        assert_eq!(
            mock.calls().last(),
            Some(&SshOp::Run(deadman::disarm_command("paperless", "sudo"))),
            "nothing must be left armed on the Host once the App is back up"
        );
    }

    #[test]
    fn backup_re_arms_before_every_individual_rsync_transfer() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut params = HashMap::new();
        params.insert("include_music".to_string(), true);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "navidrome",
                &navidrome_recipe(),
                Path::new("/tmp/dest"),
                &params,
                &mut progress,
            )
            .unwrap();

        let calls = mock.calls();
        let rsync_positions: Vec<usize> = calls
            .iter()
            .enumerate()
            .filter_map(|(i, c)| matches!(c, SshOp::RsyncFrom { .. }).then_some(i))
            .collect();
        assert_eq!(rsync_positions.len(), 2, "both paths must have rsynced");
        let arm_before = |idx: usize| {
            calls[..idx]
                .iter()
                .rev()
                .find(|c| !matches!(c, SshOp::RsyncFrom { .. }))
        };
        assert!(
            matches!(arm_before(rsync_positions[0]), Some(SshOp::RunDetached(_))),
            "the first rsync must be immediately preceded by a re-arm"
        );
        assert!(
            matches!(arm_before(rsync_positions[1]), Some(SshOp::RunDetached(_))),
            "the second rsync must get its own re-arm, not share the first one's"
        );
    }

    #[test]
    fn backup_arms_the_recipes_units_in_their_declared_quiesce_order() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        executor
            .backup(
                "bichon",
                &shipped_bichon_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .unwrap();

        let arm = detached_calls(&mock.calls())
            .into_iter()
            .next()
            .expect("bichon has services and must arm a deadman");
        let timer_pos = arm.find("systemctl start bichon-archive.timer;").unwrap();
        let server_pos = arm.find("systemctl start bichon;").unwrap();
        assert!(
            timer_pos < server_pos,
            "the fire path must replay the same order the executor's own \
             restart uses: {arm}"
        );
    }

    #[test]
    fn backup_disarms_the_deadman_even_when_the_dump_fails() {
        let mock = MockSshSession::new();
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
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();
        let result = executor.backup(
            "paperless",
            &paperless_recipe(),
            Path::new("/tmp/dest"),
            &HashMap::new(),
            &mut progress,
        );
        assert!(result.is_err());

        assert_eq!(
            mock.calls().last(),
            Some(&SshOp::Run(deadman::disarm_command("paperless", "sudo"))),
            "a failed backup must still leave nothing armed on the Host"
        );
    }

    #[test]
    fn backup_never_touches_the_deadman_for_a_recipe_with_no_services() {
        let mock = MockSshSession::new();
        backup_with(
            &BackupRecipe {
                systemd_services: Vec::new(),
                attests: None,
                ..baikal_recipe()
            },
            &mock,
        )
        .unwrap();

        assert!(
            !mock
                .calls()
                .iter()
                .any(|c| matches!(c, SshOp::RunDetached(_))
                    || matches!(c, SshOp::Run(cmd) if cmd.contains("deadman"))),
            "there is no quiesce window to guard when nothing is quiesced"
        );
    }

    #[test]
    fn backup_warns_and_still_completes_when_a_previous_deadman_fired() {
        let mock = MockSshSession::new();
        mock.stage_run_result(crate::services::ssh::CommandResult::from_stdout(
            "Tue Sep  1 01:14:39 UTC 2026\n",
        ));
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();

        executor
            .backup(
                "paperless",
                &paperless_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .expect("a prior fire is a warning, not a reason to fail this run");

        let warnings: Vec<String> = progress
            .events()
            .into_iter()
            .filter_map(|e| match e {
                crate::services::progress::ProgressEvent::Warn(msg) => Some(msg),
                _ => None,
            })
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("paperless"), "{warnings:?}");
    }

    #[test]
    fn backup_warns_but_still_completes_when_arming_the_deadman_fails() {
        let mock = MockSshSession::new();
        mock.fail_run_detached();
        let executor = RecipeExecutor::new(&mock);
        let mut progress = crate::services::progress::MockProgress::new();

        executor
            .backup(
                "paperless",
                &paperless_recipe(),
                Path::new("/tmp/dest"),
                &HashMap::new(),
                &mut progress,
            )
            .expect("a failed arm must not block the backup itself");

        let warnings: Vec<String> = progress
            .events()
            .into_iter()
            .filter_map(|e| match e {
                crate::services::progress::ProgressEvent::Warn(msg) => Some(msg),
                _ => None,
            })
            .collect();
        assert!(
            !warnings.is_empty(),
            "a silently unguarded window defeats the whole mechanism"
        );
        assert!(warnings.iter().all(|w| w.contains("paperless")));
    }
}
