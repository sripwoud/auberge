use crate::hosts::Host;
mod transport;

use eyre::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use transport::SshTransport;

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

/// Tier 2 of SSH key resolution (docs/configuration/ssh-keys.md): hosts.toml
/// `ssh_key` values may carry `~`, which must expand before any `.exists()`
/// check.
pub fn configured_key_path(raw: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(raw).as_ref())
}

/// Full three-tier resolution: `--ssh-key` flag > hosts.toml `ssh_key` >
/// derived `~/.ssh/identities/{host}/{user}` (docs/configuration/ssh-keys.md).
pub fn resolve_ssh_key_path(host: &Host, override_key: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(key_path) = override_key {
        if !key_path.exists() {
            eyre::bail!(
                "Specified SSH key not found: {}\nCheck the path and try again",
                key_path.display()
            );
        }

        validate_key_file(&key_path)?;
        return Ok(key_path);
    }

    if let Some(ref configured_key) = host.ssh_key {
        let key_path = configured_key_path(configured_key);
        if key_path.exists() {
            validate_key_file(&key_path)?;
            return Ok(key_path);
        }
        eprintln!(
            "⚠ Warning: Configured SSH key not found: {}",
            key_path.display()
        );
        eprintln!("  Falling back to default key derivation");
    }

    let ssh_key = default_ssh_key_path(&host.user, &host.name)?;

    if !ssh_key.exists() {
        eyre::bail!(
            "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}' or configure with 'auberge host edit {}'",
            ssh_key.display(),
            host.name,
            host.user,
            host.name
        );
    }

    Ok(ssh_key)
}

fn validate_key_file(key_path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(key_path)
        .wrap_err_with(|| format!("Cannot read SSH key: {}", key_path.display()))?;

    if !metadata.is_file() {
        eyre::bail!("SSH key path is not a file: {}", key_path.display());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = metadata.permissions();
        let mode = perms.mode() & 0o777;
        if mode & 0o077 != 0 {
            eprintln!(
                "⚠ Warning: SSH key has overly permissive permissions: {:o}",
                mode
            );
            eprintln!("  Consider running: chmod 600 {}", key_path.display());
        }
    }

    Ok(())
}

pub fn identities_dir(home_dir: &Path) -> PathBuf {
    home_dir.join(".ssh/identities")
}

pub fn identity_scan_dirs(home_dir: &Path, host: &str) -> [PathBuf; 2] {
    let identities = identities_dir(home_dir);
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

/// The bound every reachability probe uses. [`SshSession::reachable`] takes the
/// timeout as a parameter because a caller may one day want a different bound;
/// today all three want this one, and a shared const makes that visible rather
/// than a coincidence three modules maintain separately.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The only way to reach a Host. Every ssh, scp and rsync the CLI issues goes
/// through an implementation of this trait, so a command that talks to a Host
/// is testable against [`MockSshSession`] without one.
pub trait SshSession {
    fn run(&self, command: &str) -> Result<CommandResult>;
    /// One argv, no shell — for a command whose arguments must not be re-split
    /// or glob-expanded by the remote shell.
    fn run_raw(&self, args: &[&str]) -> Result<CommandResult>;
    /// One bounded, non-interactive connect. Callers that are about to issue a
    /// series of commands against a Host that may have gone dark use it so a
    /// bare ssh cannot hang for the TCP timeout or block on a prompt in front
    /// of them; a success leaves the mux socket warm for what follows.
    fn reachable(&self, timeout: Duration) -> Result<()>;
    /// The transport as rsync's `-e` argument, for the callers that must build
    /// their own rsync — `sync hermes` needs `--dry-run`, which neither
    /// [`SshSession::rsync_to`] nor [`SshSession::scp_to`] can express. Flags
    /// stay at the caller; only reaching the Host comes from here.
    fn rsync_e_arg(&self) -> String;
    fn systemctl(&self, action: &str, service: &str) -> Result<()>;
    fn scp_from(&self, remote: &str, local: &Path) -> Result<()>;
    fn scp_to(&self, local: &Path, remote: &str) -> Result<()>;
    fn rsync_from(&self, remote: &str, local: &Path) -> Result<()>;
    fn rsync_to(&self, local: &Path, remote: &str) -> Result<()>;
    fn set_ownership(&self, remote: &str, user: &str, group: &str) -> Result<()>;
}

pub struct LiveSshSession<'a> {
    inner: SshTransport<'a>,
    host: &'a Host,
}

impl<'a> LiveSshSession<'a> {
    pub fn new(host: &'a Host, ssh_key: &'a Path) -> Self {
        Self {
            inner: SshTransport::new(host, ssh_key),
            host,
        }
    }
}

impl SshSession for LiveSshSession<'_> {
    fn run(&self, command: &str) -> Result<CommandResult> {
        Ok(CommandResult::from_output(self.inner.run(command)?))
    }

    fn run_raw(&self, args: &[&str]) -> Result<CommandResult> {
        Ok(CommandResult::from_output(self.inner.run_raw(args)?))
    }

    fn reachable(&self, timeout: Duration) -> Result<()> {
        let out = self.inner.probe(timeout)?;
        if out.status.success() {
            let lines =
                crate::output::subprocess_output("ssh", &String::from_utf8_lossy(&out.stderr));
            crate::output::clear_subprocess_lines(lines);
            return Ok(());
        }
        Err(unreachable_error(
            self.host,
            &String::from_utf8_lossy(&out.stderr),
        ))
    }

    fn rsync_e_arg(&self) -> String {
        self.inner.rsync_e_arg()
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

/// The one message a failed reachability probe produces. Three call sites used
/// to word this three ways — and one of them, the cross-host restore's, was the
/// only one that named the address and port a reader needs to check a firewall,
/// so the union is kept rather than the shortest.
fn unreachable_error(host: &Host, stderr: &str) -> eyre::Report {
    let stderr = stderr.trim();
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(": {}", stderr)
    };
    eyre::eyre!(
        "host {} is unreachable over ssh at {}:{}{}\nCheck the SSH key and network connectivity",
        host.name,
        host.address,
        host.port,
        detail
    )
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
    RunRaw(Vec<String>),
    Reachable(Duration),
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

    fn next_result(&self) -> CommandResult {
        self.run_results
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(CommandResult::ok)
    }
}

/// What [`MockSshSession::rsync_e_arg`] answers: a fixed string, so a fence over
/// a hand-built rsync asserts the `-e` came from the seam without asserting the
/// live transport's argv, which has its own tests.
#[cfg(test)]
pub const MOCK_RSYNC_E_ARG: &str = "ssh <mock>";

/// The Host a mock probe words its failure against, so [`unreachable_error`] is
/// exercised by the same code path the live session takes.
#[cfg(test)]
fn mock_host() -> Host {
    Host {
        name: "mock".to_string(),
        address: "192.0.2.9".to_string(),
        user: "deploy".to_string(),
        port: 22,
        ssh_key: None,
        tags: vec![],
        description: None,
        python_interpreter: None,
        become_method: "sudo".to_string(),
        tailscale_ip: None,
    }
}

#[cfg(test)]
impl SshSession for MockSshSession {
    fn run(&self, command: &str) -> Result<CommandResult> {
        self.calls
            .borrow_mut()
            .push(SshOp::Run(command.to_string()));
        Ok(self.next_result())
    }

    fn run_raw(&self, args: &[&str]) -> Result<CommandResult> {
        self.calls
            .borrow_mut()
            .push(SshOp::RunRaw(args.iter().map(|a| a.to_string()).collect()));
        Ok(self.next_result())
    }

    fn reachable(&self, timeout: Duration) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::Reachable(timeout));
        let result = self.next_result();
        if result.success {
            return Ok(());
        }
        Err(unreachable_error(&mock_host(), &result.stderr_str()))
    }

    fn rsync_e_arg(&self) -> String {
        MOCK_RSYNC_E_ARG.to_string()
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
        }
    }

    #[test]
    fn test_resolve_ssh_key_path_returns_existing_configured_key() {
        let tmp = tempfile::tempdir().unwrap();
        let key = tmp.path().join("key");
        std::fs::write(&key, b"k").unwrap();
        let host = Host {
            ssh_key: Some(key.display().to_string()),
            ..test_host()
        };
        assert_eq!(resolve_ssh_key_path(&host, None).unwrap(), key);
    }

    #[test]
    fn test_resolve_ssh_key_path_falls_back_to_derivation_when_configured_key_missing() {
        let host = Host {
            name: "no-such-host-518".to_string(),
            ssh_key: Some("~/no-such-key-518".to_string()),
            ..test_host()
        };
        let err = resolve_ssh_key_path(&host, None).unwrap_err().to_string();
        assert!(
            err.contains(".ssh/identities/no-such-host-518/deploy"),
            "{err}"
        );
        assert!(!err.contains('~'), "{err}");
    }

    #[test]
    fn test_configured_key_path_expands_tilde_to_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            configured_key_path("~/.ssh/identities/auberge/ansible"),
            home.join(".ssh/identities/auberge/ansible")
        );
    }

    #[test]
    fn test_configured_key_path_keeps_absolute_paths() {
        assert_eq!(
            configured_key_path("/etc/keys/ansible"),
            PathBuf::from("/etc/keys/ansible")
        );
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
    fn test_mock_records_run_raw_calls() {
        let mock = MockSshSession::new();
        let _ = mock
            .run_raw(&["systemctl", "list-unit-files", "radio.service"])
            .unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::RunRaw(vec![
                "systemctl".to_string(),
                "list-unit-files".to_string(),
                "radio.service".to_string(),
            ])]
        );
    }

    #[test]
    fn test_mock_records_reachable_with_the_timeout_it_was_given() {
        let mock = MockSshSession::new();
        mock.reachable(Duration::from_secs(10)).unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Reachable(Duration::from_secs(10))]
        );
    }

    #[test]
    fn test_reachable_fails_when_the_probe_does() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(255),
            stdout: Vec::new(),
            stderr: b"Connection timed out".to_vec(),
        });
        let err = mock.reachable(Duration::from_secs(1)).unwrap_err();
        assert!(err.to_string().contains("unreachable over ssh"), "{err}");
    }

    #[test]
    fn test_unreachable_error_names_the_host_address_and_port() {
        let host = Host {
            name: "auberge".to_string(),
            address: "203.0.113.7".to_string(),
            port: 2222,
            ..test_host()
        };
        let msg = unreachable_error(&host, "").to_string();
        assert!(
            msg.contains("host auberge is unreachable over ssh"),
            "{msg}"
        );
        assert!(msg.contains("203.0.113.7:2222"), "{msg}");
        assert!(
            msg.contains("Check the SSH key and network connectivity"),
            "{msg}"
        );
    }

    #[test]
    fn test_unreachable_error_carries_the_probe_stderr() {
        let msg = unreachable_error(&test_host(), "  Permission denied (publickey).\n").to_string();
        assert!(msg.contains(": Permission denied (publickey)."), "{msg}");
    }

    #[test]
    fn test_unreachable_error_says_nothing_extra_when_stderr_is_empty() {
        let msg = unreachable_error(&test_host(), "   ").to_string();
        let first_line = msg.lines().next().unwrap();
        assert!(first_line.ends_with("192.0.2.1:2222"), "{first_line}");
    }

    #[test]
    fn test_mock_rsync_e_arg_is_the_fixed_seam_marker() {
        assert_eq!(MockSshSession::new().rsync_e_arg(), MOCK_RSYNC_E_ARG);
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
