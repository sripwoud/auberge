use crate::hosts::Host;
use crate::ssh_session::SshSession as InnerSession;
use eyre::{Context, Result};
use std::path::Path;
use std::process::Command;

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
            .arg(format!("{}/", local.display()))
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
