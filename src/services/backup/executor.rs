use crate::models::playbook_meta::BackupRecipe;
use crate::services::backup::ssh::SshSession;
use eyre::Result;
use std::collections::HashMap;
use std::path::Path;

pub struct RecipeExecutor<'a, S: SshSession + ?Sized> {
    session: &'a S,
}

impl<'a, S: SshSession + ?Sized> RecipeExecutor<'a, S> {
    pub fn new(session: &'a S) -> Self {
        Self { session }
    }

    pub fn backup(
        &self,
        recipe: &BackupRecipe,
        dest_dir: &Path,
        parameters: &HashMap<String, bool>,
    ) -> Result<()> {
        let mut stopped: Vec<&str> = Vec::new();
        for service in &recipe.systemd_services {
            if let Err(e) = self.session.systemctl("stop", service) {
                self.restart_all(&stopped);
                return Err(e);
            }
            stopped.push(service);
        }

        let result = (|| -> Result<()> {
            if let Some(db) = &recipe.db {
                let cmd = format!(
                    "sudo -u postgres pg_dump -Fc {} > {}",
                    db.name, db.dump_path
                );
                let dump = self.session.run(&cmd)?;
                if !dump.success {
                    let _ = self.session.run(&format!("rm -f {}", db.dump_path));
                    eyre::bail!(
                        "pg_dump failed for {}: {}",
                        db.name,
                        dump.stderr_str().trim()
                    );
                }
            }

            let paths = recipe.effective_paths(parameters);
            for path in &paths {
                self.session.rsync_from(path, dest_dir)?;
            }

            if let Some(db) = &recipe.db {
                let local_dump = dest_dir.join("db.dump");
                self.session.scp_from(&db.dump_path, &local_dump)?;
                let _ = self.session.run(&format!("rm -f {}", db.dump_path));
            }

            Ok(())
        })();

        let restart_failures = self.restart_all_collecting(&stopped);

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

    pub fn restore(
        &self,
        recipe: &BackupRecipe,
        source_dir: &Path,
        parameters: &HashMap<String, bool>,
    ) -> Result<()> {
        let mut stopped: Vec<&str> = Vec::new();
        for service in &recipe.systemd_services {
            if let Err(e) = self.session.systemctl("stop", service) {
                self.restart_all(&stopped);
                return Err(e);
            }
            stopped.push(service);
        }

        let result = (|| -> Result<()> {
            let paths = recipe.effective_paths(parameters);
            for path in &paths {
                let local_source = source_dir.join(path.trim_start_matches('/'));
                self.session.rsync_to(&local_source, path)?;
            }

            if let Some((user, group)) = &recipe.owner {
                for path in &paths {
                    self.session.set_ownership(path, user, group)?;
                }
            }

            if let Some(db) = &recipe.db {
                let local_dump = source_dir.join("db.dump");
                if local_dump.exists() {
                    self.session.scp_to(&local_dump, &db.dump_path)?;
                    self.session.run(&format!("chmod 644 {}", db.dump_path))?;
                    let cmd = format!(
                        "sudo -u postgres pg_restore --clean --if-exists -d {} {} 2>&1",
                        db.name, db.dump_path
                    );
                    let restore = self.session.run(&cmd)?;
                    let _ = self.session.run(&format!("rm -f {}", db.dump_path));
                    if !restore.success && !is_warnings_only(&restore.stdout_str()) {
                        eyre::bail!("pg_restore failed: {}", restore.stdout_str().trim());
                    }
                }
            }

            if let Some(cmd) = &recipe.post_restore_command {
                let post = self.session.run(cmd)?;
                if !post.success {
                    eyre::bail!("post_restore_command failed: {}", post.stderr_str().trim());
                }
            }

            Ok(())
        })();

        let restart_failures = self.restart_all_collecting(&stopped);

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

fn is_warnings_only(text: &str) -> bool {
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
    use crate::models::playbook_meta::{BackupParameter, DbRecipe};
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

    fn paperless_recipe() -> BackupRecipe {
        BackupRecipe {
            systemd_services: vec!["paperless-webserver".to_string()],
            paths: vec!["/opt/paperless/data".to_string()],
            owner: Some(("paperless".to_string(), "paperless".to_string())),
            db: Some(DbRecipe {
                name: "paperless".to_string(),
                dump_path: "/tmp/paperless_db.dump".to_string(),
            }),
            post_restore_command: Some("sudo -u paperless ./manage.py migrate".to_string()),
            parameters: HashMap::new(),
        }
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
        }
    }

    #[test]
    fn test_backup_no_services_just_rsyncs_paths() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        executor
            .backup(&baikal_recipe(), Path::new("/tmp/dest"), &HashMap::new())
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
        };
        let executor = RecipeExecutor::new(&mock);
        executor
            .backup(&recipe, Path::new("/tmp/dest"), &HashMap::new())
            .unwrap();

        let calls = mock.calls();
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
        executor
            .backup(&paperless_recipe(), Path::new("/tmp/dest"), &HashMap::new())
            .unwrap();

        let calls = mock.calls();
        assert_eq!(
            calls[0],
            SshOp::Systemctl {
                action: "stop".to_string(),
                service: "paperless-webserver".to_string(),
            }
        );
        match &calls[1] {
            SshOp::Run(cmd) => {
                assert!(cmd.contains("pg_dump"));
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
    fn test_backup_with_include_music_parameter_adds_path() {
        let mock = MockSshSession::new();
        let executor = RecipeExecutor::new(&mock);
        let mut params = HashMap::new();
        params.insert("include_music".to_string(), true);
        executor
            .backup(&navidrome_recipe(), Path::new("/tmp/dest"), &params)
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
        executor
            .backup(&navidrome_recipe(), Path::new("/tmp/dest"), &HashMap::new())
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
        };
        executor
            .restore(&recipe, Path::new("/tmp/source"), &HashMap::new())
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
        executor
            .restore(&paperless_recipe(), tmp.path(), &HashMap::new())
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
        executor
            .restore(&paperless_recipe(), tmp.path(), &HashMap::new())
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
    fn test_backup_failed_pg_dump_still_restarts_services() {
        let mock = MockSshSession::new();
        mock.stage_run_result(crate::services::backup::ssh::CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"connection refused".to_vec(),
        });
        let executor = RecipeExecutor::new(&mock);
        let result = executor.backup(&paperless_recipe(), Path::new("/tmp/dest"), &HashMap::new());
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
}
