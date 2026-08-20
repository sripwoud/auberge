use crate::hosts::Host;
use crate::ssh_session::SshSession as InnerSession;
use eyre::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn default_ssh_key_path(user: &str, host: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    let [host_dir, _] = identity_scan_dirs(&home, host);
    Ok(host_dir.join(user))
}

pub fn legacy_ssh_key_path(user: &str, host: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    Ok(home
        .join(".ssh/identities")
        .join(format!("{}_{}", user, host)))
}

pub fn identity_scan_dirs(home_dir: &Path, host: &str) -> [PathBuf; 2] {
    let identities = home_dir.join(".ssh/identities");
    [identities.join(host), identities]
}

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
    #[allow(dead_code)]
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandResult {
    #[allow(dead_code)]
    pub fn ok() -> Self {
        Self {
            success: true,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    pub fn from_output(out: std::process::Output) -> Self {
        Self {
            success: out.status.success(),
            exit_code: out.status.code(),
            stdout: out.stdout,
            stderr: out.stderr,
        }
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

pub trait SshSession {
    fn run(&self, command: &str) -> Result<CommandResult>;
    fn systemctl(&self, action: &str, service: &str) -> Result<()>;
    fn scp_from(&self, remote: &str, local: &Path) -> Result<()>;
    fn scp_to(&self, local: &Path, remote: &str) -> Result<()>;
    fn rsync_from(&self, remote: &str, local: &Path) -> Result<()>;
    fn rsync_to(&self, local: &Path, remote: &str) -> Result<()>;
    fn set_ownership(&self, remote: &str, user: &str, group: &str) -> Result<()>;
}

pub struct LiveSshSession<'a> {
    inner: InnerSession<'a>,
    host: &'a Host,
}

impl<'a> LiveSshSession<'a> {
    pub fn new(host: &'a Host, ssh_key: &'a Path) -> Self {
        Self {
            inner: InnerSession::new(host, ssh_key),
            host,
        }
    }
}

impl SshSession for LiveSshSession<'_> {
    fn run(&self, command: &str) -> Result<CommandResult> {
        Ok(CommandResult::from_output(self.inner.run(command)?))
    }

    fn systemctl(&self, action: &str, service: &str) -> Result<()> {
        self.inner.systemctl(action, service)
    }

    fn scp_from(&self, remote: &str, local: &Path) -> Result<()> {
        self.inner.scp_from(remote, local)
    }

    fn scp_to(&self, local: &Path, remote: &str) -> Result<()> {
        self.inner.scp_to(local, remote)
    }

    fn rsync_from(&self, remote: &str, local: &Path) -> Result<()> {
        let out = Command::new("rsync")
            .arg("-az")
            .arg("--relative")
            .arg("--rsync-path=sudo rsync")
            .arg("-e")
            .arg(self.inner.rsync_e_arg())
            .arg(format!(
                "{}@{}:{}",
                self.host.user, self.host.address, remote
            ))
            .arg(local)
            .output()
            .wrap_err("Failed to execute rsync")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.trim().is_empty() {
                eyre::bail!("rsync failed for {}", remote);
            }
            eyre::bail!("rsync failed for {}: {}", remote, stderr.trim());
        }
        Ok(())
    }

    fn rsync_to(&self, local: &Path, remote: &str) -> Result<()> {
        let out = Command::new("rsync")
            .arg("-az")
            .arg("--delete")
            .arg("--rsync-path=sudo rsync")
            .arg("-e")
            .arg(self.inner.rsync_e_arg())
            .arg(rsync_source_arg(local))
            .arg(format!(
                "{}@{}:{}",
                self.host.user, self.host.address, remote
            ))
            .output()
            .wrap_err("Failed to execute rsync")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.trim().is_empty() {
                eyre::bail!("rsync failed for {}", remote);
            }
            eyre::bail!("rsync failed for {}: {}", remote, stderr.trim());
        }
        Ok(())
    }

    fn set_ownership(&self, remote: &str, user: &str, group: &str) -> Result<()> {
        let cmd = format!("sudo chown -R {}:{} {}", user, group, remote);
        let result = self.run(&cmd)?;
        if !result.success {
            eyre::bail!("chown -R {}:{} {} failed", user, group, remote);
        }
        Ok(())
    }
}

/// A trailing slash means "contents of" to rsync — required for directory
/// sources, fatal for single-file sources (Backup Recipe paths can be either).
fn rsync_source_arg(local: &Path) -> String {
    if local.is_dir() {
        format!("{}/", local.display())
    } else {
        local.display().to_string()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshOp {
    Run(String),
    Systemctl {
        action: String,
        service: String,
    },
    ScpFrom {
        remote: String,
        local: std::path::PathBuf,
    },
    ScpTo {
        local: std::path::PathBuf,
        remote: String,
    },
    RsyncFrom {
        remote: String,
        local: std::path::PathBuf,
    },
    RsyncTo {
        local: std::path::PathBuf,
        remote: String,
    },
    SetOwnership {
        remote: String,
        user: String,
        group: String,
    },
}

#[cfg(test)]
pub struct MockSshSession {
    calls: std::cell::RefCell<Vec<SshOp>>,
    run_results: std::cell::RefCell<std::collections::VecDeque<CommandResult>>,
}

#[cfg(test)]
impl MockSshSession {
    pub fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            run_results: std::cell::RefCell::new(std::collections::VecDeque::new()),
        }
    }

    pub fn stage_run_result(&self, result: CommandResult) {
        self.run_results.borrow_mut().push_back(result);
    }

    pub fn calls(&self) -> Vec<SshOp> {
        self.calls.borrow().clone()
    }
}

#[cfg(test)]
impl SshSession for MockSshSession {
    fn run(&self, command: &str) -> Result<CommandResult> {
        self.calls
            .borrow_mut()
            .push(SshOp::Run(command.to_string()));
        Ok(self
            .run_results
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(CommandResult::ok))
    }

    fn systemctl(&self, action: &str, service: &str) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::Systemctl {
            action: action.to_string(),
            service: service.to_string(),
        });
        Ok(())
    }

    fn scp_from(&self, remote: &str, local: &Path) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::ScpFrom {
            remote: remote.to_string(),
            local: local.to_path_buf(),
        });
        Ok(())
    }

    fn scp_to(&self, local: &Path, remote: &str) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::ScpTo {
            local: local.to_path_buf(),
            remote: remote.to_string(),
        });
        Ok(())
    }

    fn rsync_from(&self, remote: &str, local: &Path) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::RsyncFrom {
            remote: remote.to_string(),
            local: local.to_path_buf(),
        });
        Ok(())
    }

    fn rsync_to(&self, local: &Path, remote: &str) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::RsyncTo {
            local: local.to_path_buf(),
            remote: remote.to_string(),
        });
        Ok(())
    }

    fn set_ownership(&self, remote: &str, user: &str, group: &str) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::SetOwnership {
            remote: remote.to_string(),
            user: user.to_string(),
            group: group.to_string(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsync_source_arg_appends_slash_to_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let arg = rsync_source_arg(tmp.path());
        assert!(arg.ends_with('/'), "{arg}");
    }

    #[test]
    fn test_rsync_source_arg_keeps_file_path_bare() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.xml");
        std::fs::write(&file, b"x").unwrap();
        let arg = rsync_source_arg(&file);
        assert!(!arg.ends_with('/'), "{arg}");
        assert_eq!(arg, file.display().to_string());
    }

    #[test]
    fn test_default_ssh_key_path_is_host_scoped() {
        let path = default_ssh_key_path("ansible", "vieille-auberge").unwrap();
        assert!(path.ends_with(".ssh/identities/vieille-auberge/ansible"));
    }

    #[test]
    fn test_legacy_ssh_key_path_uses_flat_underscore_layout() {
        let path = legacy_ssh_key_path("ansible", "auberge").unwrap();
        assert!(path.ends_with(".ssh/identities/ansible_auberge"));
    }

    #[test]
    fn test_identity_scan_dirs_lists_host_dir_before_flat_dir() {
        let home = std::path::Path::new("/home/x");
        let [host_dir, flat_dir] = identity_scan_dirs(home, "myserver");
        assert_eq!(
            host_dir,
            std::path::Path::new("/home/x/.ssh/identities/myserver")
        );
        assert_eq!(flat_dir, std::path::Path::new("/home/x/.ssh/identities"));
    }

    #[test]
    fn test_default_ssh_key_path_places_user_as_filename() {
        let path = default_ssh_key_path("sripwoud", "auberge").unwrap();
        assert_eq!(path.file_name().unwrap(), "sripwoud");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "auberge");
    }

    #[test]
    fn test_command_result_ok_is_success() {
        let r = CommandResult::ok();
        assert!(r.success);
        assert_eq!(r.exit_code, Some(0));
        assert!(r.stdout.is_empty());
        assert!(r.stderr.is_empty());
    }

    #[test]
    fn test_mock_records_run_calls() {
        let mock = MockSshSession::new();
        let _ = mock.run("echo hello").unwrap();
        assert_eq!(mock.calls(), vec![SshOp::Run("echo hello".to_string())]);
    }

    #[test]
    fn test_mock_records_systemctl_calls() {
        let mock = MockSshSession::new();
        mock.systemctl("stop", "paperless-webserver").unwrap();
        mock.systemctl("start", "paperless-webserver").unwrap();
        assert_eq!(
            mock.calls(),
            vec![
                SshOp::Systemctl {
                    action: "stop".to_string(),
                    service: "paperless-webserver".to_string(),
                },
                SshOp::Systemctl {
                    action: "start".to_string(),
                    service: "paperless-webserver".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_mock_records_rsync_from_calls() {
        let mock = MockSshSession::new();
        mock.rsync_from("/var/lib/freshrss", Path::new("/tmp/staging"))
            .unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::RsyncFrom {
                remote: "/var/lib/freshrss".to_string(),
                local: std::path::PathBuf::from("/tmp/staging"),
            }]
        );
    }

    #[test]
    fn test_mock_returns_staged_run_result() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: b"oops".to_vec(),
            stderr: b"error".to_vec(),
        });
        let result = mock.run("test").unwrap();
        assert!(!result.success);
        assert_eq!(result.stdout_str(), "oops");
        assert_eq!(result.stderr_str(), "error");
    }

    #[test]
    fn test_mock_returns_default_ok_when_no_staged_results() {
        let mock = MockSshSession::new();
        let result = mock.run("anything").unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_mock_records_set_ownership() {
        let mock = MockSshSession::new();
        mock.set_ownership("/opt/paperless", "paperless", "paperless")
            .unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::SetOwnership {
                remote: "/opt/paperless".to_string(),
                user: "paperless".to_string(),
                group: "paperless".to_string(),
            }]
        );
    }
}
