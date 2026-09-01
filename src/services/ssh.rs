use crate::hosts::Host;
use crate::services::route::Route;
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

/// The exit status ssh(1) reserves for its own failures, documented under
/// EXIT STATUS: "ssh exits with the exit status of the remote command or with
/// 255 if an error occurred."
const SSH_TRANSPORT_EXIT: i32 = 255;

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
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

    /// A successful result carrying `text` on stdout — the shape almost every
    /// staged result wants, and previously re-declared in each test mod that
    /// needed it.
    #[cfg(test)]
    pub fn from_stdout(text: &str) -> Self {
        Self {
            success: true,
            exit_code: Some(0),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    /// An ssh that never connected: 255, nothing on stdout, its own complaint
    /// on stderr. Staged by both this module's tests and the restore
    /// pre-flight's, so it is declared once rather than hand-built at each.
    #[cfg(test)]
    pub fn transport_failure(stderr: &str) -> Self {
        Self {
            success: false,
            exit_code: Some(SSH_TRANSPORT_EXIT),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// A failed result carrying `stderr` — the other half of the pair.
    #[cfg(test)]
    pub fn from_stderr(stderr: &str) -> Self {
        Self {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// `true` when the *transport* failed rather than the remote command
    /// answering. The stdout half is what keeps a remote command's own 255
    /// out: output proves the remote ran.
    ///
    /// A remote command that exits 255 having printed nothing is the one case
    /// this cannot separate — ssh multiplexes both onto one exit status.
    fn ssh_transport_failed(&self) -> bool {
        self.exit_code == Some(SSH_TRANSPORT_EXIT) && self.stdout.is_empty()
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// The bound every reachability probe uses.
///
/// Two of the three probes this replaced carried ten seconds and the third
/// carried none at all, so the const is what makes the agreement real rather
/// than a coincidence maintained in three modules. The timeout stays a
/// parameter of [`SshSession::reachable`] — that is the signature #669
/// settled on — but no caller has yet wanted a different one.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The only way to reach a Host. Every ssh, scp and rsync the CLI issues goes
/// through an implementation of this trait, so a command that talks to a Host
/// is testable against [`MockSshSession`] without one.
pub trait SshSession {
    fn run(&self, command: &str) -> Result<CommandResult>;
    /// Launches `command` without waiting for it to finish — see
    /// [`SshTransport::spawn`] for why this exists alongside every other,
    /// blocking method here: arming a Host-side deadman (ADR-0066) must not
    /// depend on the ssh round trip itself completing quickly.
    fn run_detached(&self, command: &str) -> Result<()>;
    /// A command given as pieces rather than as one string the caller joined.
    ///
    /// This buys nothing on the *remote* side — ssh(1) appends its arguments
    /// "separated by spaces, before it is sent to the server", so the remote
    /// login shell parses the result either way, and a value carrying a space
    /// or a glob is no safer here than in [`SshSession::run`]. What it saves is
    /// the local `format!`, for the callers whose command is already a list.
    ///
    /// Unlike [`SshSession::run`], an `Ok` here means the *Host* answered: a
    /// transport failure is an `Err`, worded by [`unreachable_error`] like
    /// every other. `run`'s own callers each already turn a non-zero exit into
    /// their own domain error, so re-contracting the twenty-five of them is a
    /// separate decision from fixing the one caller here (#693).
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
    /// The Host's configured escalation command (`sudo` by default, see
    /// #776), for a caller building its own privileged command line rather
    /// than going through [`SshSession::systemctl`], [`SshSession::rsync_to`]
    /// or [`SshSession::set_ownership`]'s own escalation — the Host-side
    /// deadman (ADR-0066) arms via `systemd-run` outside all three of those.
    fn become_method(&self) -> &str;
    fn systemctl(&self, action: &str, service: &str) -> Result<()>;
    fn scp_from(&self, remote: &str, local: &Path) -> Result<()>;
    fn scp_to(&self, local: &Path, remote: &str) -> Result<()>;
    fn rsync_from(&self, remote: &str, local: &Path) -> Result<()>;
    fn rsync_to(&self, local: &Path, remote: &str) -> Result<()>;
    fn set_ownership(&self, remote: &str, user: &str, group: &str) -> Result<()>;
}

pub struct LiveSshSession<'a> {
    inner: SshTransport<'a>,
    route: &'a Route,
    become_method: &'a str,
}

impl<'a> LiveSshSession<'a> {
    pub fn new(route: &'a Route, become_method: &'a str) -> Result<Self> {
        Ok(Self {
            inner: SshTransport::new(route, become_method)?,
            route,
            become_method,
        })
    }

    /// A session for a Host no trust is established with yet. See
    /// [`transport::Reach::FirstContact`] for what it gives up and why.
    pub fn first_contact(route: &'a Route, become_method: &'a str) -> Result<Self> {
        Ok(Self {
            inner: SshTransport::first_contact(route, become_method)?,
            route,
            become_method,
        })
    }

    /// rsync's own escalation flag, using the acting Host's `become_method`
    /// (`sudo` by default, see #776).
    fn rsync_path_arg(&self) -> String {
        format!("--rsync-path={} rsync", self.become_method)
    }

    /// The remote chown, escalated with the acting Host's `become_method`.
    fn chown_command(&self, remote: &str, user: &str, group: &str) -> String {
        format!(
            "{} chown -R {}:{} {}",
            self.become_method, user, group, remote
        )
    }
}

impl SshSession for LiveSshSession<'_> {
    fn run(&self, command: &str) -> Result<CommandResult> {
        Ok(CommandResult::from_output(self.inner.run(command)?))
    }

    fn run_detached(&self, command: &str) -> Result<()> {
        self.inner.spawn(command)
    }

    fn run_raw(&self, args: &[&str]) -> Result<CommandResult> {
        let result = CommandResult::from_output(self.inner.run_raw(args)?);
        if result.ssh_transport_failed() {
            return Err(unreachable_error(self.route, &result.stderr_str()));
        }
        Ok(result)
    }

    fn reachable(&self, timeout: Duration) -> Result<()> {
        let out = self.inner.probe(timeout)?;
        if out.status.success() {
            return Ok(());
        }
        Err(unreachable_error(
            self.route,
            &String::from_utf8_lossy(&out.stderr),
        ))
    }

    fn rsync_e_arg(&self) -> String {
        self.inner.rsync_e_arg()
    }

    fn become_method(&self) -> &str {
        self.become_method
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
            .arg(self.rsync_path_arg())
            .arg("-e")
            .arg(self.inner.rsync_e_arg())
            .arg(format!(
                "{}@{}:{}",
                self.route.user, self.route.address, remote
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
            .arg(self.rsync_path_arg())
            .arg("-e")
            .arg(self.inner.rsync_e_arg())
            .arg(rsync_source_arg(local))
            .arg(format!(
                "{}@{}:{}",
                self.route.user, self.route.address, remote
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
        let cmd = self.chown_command(remote, user, group);
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
///
/// Named after `route.alias` rather than a `Host`: the alias is the Host's
/// name today (#785 gives it independent meaning), and nothing here needs the
/// declaration itself to report a failed connection.
fn unreachable_error(route: &Route, stderr: &str) -> eyre::Report {
    let stderr = stderr.trim();
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(": {}", stderr)
    };
    eyre::eyre!(
        "host {} is unreachable over ssh at {}:{}{}\nCheck the SSH key and network connectivity",
        route.alias,
        route.address,
        route.port,
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
    RunDetached(String),
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
    fail_run_detached: std::cell::Cell<bool>,
    fail_systemctl_action: std::cell::RefCell<Option<String>>,
    fail_rsync_to: std::cell::Cell<bool>,
    become_method: String,
}

#[cfg(test)]
impl MockSshSession {
    pub fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            run_results: std::cell::RefCell::new(std::collections::VecDeque::new()),
            fail_run_detached: std::cell::Cell::new(false),
            fail_systemctl_action: std::cell::RefCell::new(None),
            fail_rsync_to: std::cell::Cell::new(false),
            become_method: "sudo".to_string(),
        }
    }

    /// A mock standing in for a Host configured with a non-default
    /// escalation command — for a caller building its own privileged command
    /// line to test against (deadman arm/disarm; ADR-0066).
    pub fn with_become_method(method: &str) -> Self {
        Self {
            become_method: method.to_string(),
            ..Self::new()
        }
    }

    pub fn stage_run_result(&self, result: CommandResult) {
        self.run_results.borrow_mut().push_back(result);
    }

    /// Makes every subsequent `run_detached` call fail, as if the local
    /// `ssh` process could not even be launched — the one way arming a
    /// deadman (ADR-0066) can fail.
    pub fn fail_run_detached(&self) {
        self.fail_run_detached.set(true);
    }

    /// Makes every `systemctl <action> ...` call fail, leaving every other
    /// action working — a unit that will not stop while the rest of the Host
    /// is fine, which is what separates a failed-quiesce exit path from a
    /// Host that is simply unreachable.
    pub fn fail_systemctl(&self, action: &str) {
        *self.fail_systemctl_action.borrow_mut() = Some(action.to_string());
    }

    /// Makes every push to the Host fail, as a restore whose transfer dies
    /// part-way through does.
    pub fn fail_rsync_to(&self) {
        self.fail_rsync_to.set(true);
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

/// A stand-in Host for [`MockSshSession::reachable`] to word its failure
/// against. The mock has no Host of its own, and the alternative — a second
/// wording for the mock — is the thing [`unreachable_error`] exists to prevent.
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
        tailnet_tag: None,
        unknown: toml::Table::new(),
    }
}

/// [`mock_host`], resolved — the mock's own stand-in for the Route
/// [`unreachable_error`] now takes.
#[cfg(test)]
fn mock_route() -> Route {
    crate::services::route::resolve(&mock_host(), Some(PathBuf::from("/tmp/key")))
}

#[cfg(test)]
impl SshSession for MockSshSession {
    fn run(&self, command: &str) -> Result<CommandResult> {
        self.calls
            .borrow_mut()
            .push(SshOp::Run(command.to_string()));
        Ok(self.next_result())
    }

    fn run_detached(&self, command: &str) -> Result<()> {
        self.calls
            .borrow_mut()
            .push(SshOp::RunDetached(command.to_string()));
        if self.fail_run_detached.get() {
            eyre::bail!("Failed to launch detached SSH command");
        }
        Ok(())
    }

    fn run_raw(&self, args: &[&str]) -> Result<CommandResult> {
        self.calls
            .borrow_mut()
            .push(SshOp::RunRaw(args.iter().map(|a| a.to_string()).collect()));
        let result = self.next_result();
        if result.ssh_transport_failed() {
            return Err(unreachable_error(&mock_route(), &result.stderr_str()));
        }
        Ok(result)
    }

    fn reachable(&self, timeout: Duration) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::Reachable(timeout));
        let result = self.next_result();
        if result.success {
            return Ok(());
        }
        Err(unreachable_error(&mock_route(), &result.stderr_str()))
    }

    fn rsync_e_arg(&self) -> String {
        MOCK_RSYNC_E_ARG.to_string()
    }

    fn become_method(&self) -> &str {
        &self.become_method
    }

    fn systemctl(&self, action: &str, service: &str) -> Result<()> {
        self.calls.borrow_mut().push(SshOp::Systemctl {
            action: action.to_string(),
            service: service.to_string(),
        });
        if self.fail_systemctl_action.borrow().as_deref() == Some(action) {
            eyre::bail!("Failed to {action} {service}");
        }
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
        if self.fail_rsync_to.get() {
            eyre::bail!("rsync to {remote} failed");
        }
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
            port: 2222,
            ..mock_host()
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
    fn test_mock_records_run_detached_calls() {
        let mock = MockSshSession::new();
        mock.run_detached("systemd-run --on-active=3600 --unit=x true")
            .unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::RunDetached(
                "systemd-run --on-active=3600 --unit=x true".to_string()
            )]
        );
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
        let route = crate::services::route::resolve(&host, None);
        let msg = unreachable_error(&route, "").to_string();
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
        let route = crate::services::route::resolve(&test_host(), None);
        let msg = unreachable_error(&route, "  Permission denied (publickey).\n").to_string();
        assert!(msg.contains(": Permission denied (publickey)."), "{msg}");
    }

    #[test]
    fn test_unreachable_error_says_nothing_extra_when_stderr_is_empty() {
        let route = crate::services::route::resolve(&test_host(), None);
        let msg = unreachable_error(&route, "   ").to_string();
        let first_line = msg.lines().next().unwrap();
        assert!(first_line.ends_with("192.0.2.9:2222"), "{first_line}");
    }

    #[test]
    fn test_rsync_path_arg_defaults_to_sudo() {
        let host = test_host();
        let route = crate::services::route::resolve(&host, Some(PathBuf::from("/tmp/key")));
        let session = LiveSshSession::new(&route, &host.become_method).unwrap();
        assert_eq!(session.rsync_path_arg(), "--rsync-path=sudo rsync");
    }

    #[test]
    fn test_rsync_path_arg_uses_configured_become_method() {
        let host = Host {
            become_method: "doas".to_string(),
            ..test_host()
        };
        let route = crate::services::route::resolve(&host, Some(PathBuf::from("/tmp/key")));
        let session = LiveSshSession::new(&route, &host.become_method).unwrap();
        assert_eq!(session.rsync_path_arg(), "--rsync-path=doas rsync");
    }

    #[test]
    fn test_chown_command_defaults_to_sudo() {
        let host = test_host();
        let route = crate::services::route::resolve(&host, Some(PathBuf::from("/tmp/key")));
        let session = LiveSshSession::new(&route, &host.become_method).unwrap();
        assert_eq!(
            session.chown_command("/opt/paperless", "paperless", "paperless"),
            "sudo chown -R paperless:paperless /opt/paperless"
        );
    }

    #[test]
    fn test_chown_command_uses_configured_become_method() {
        let host = Host {
            become_method: "doas".to_string(),
            ..test_host()
        };
        let route = crate::services::route::resolve(&host, Some(PathBuf::from("/tmp/key")));
        let session = LiveSshSession::new(&route, &host.become_method).unwrap();
        assert_eq!(
            session.chown_command("/opt/paperless", "paperless", "paperless"),
            "doas chown -R paperless:paperless /opt/paperless"
        );
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

    #[test]
    fn ssh_transport_failed_on_255_with_no_output() {
        assert!(
            CommandResult::transport_failure("ssh: connect to host: Connection refused")
                .ssh_transport_failed()
        );
    }

    /// Output on stdout proves the remote ran, so a 255 alongside it is the
    /// remote command's own — the stdout half of the test is load-bearing.
    #[test]
    fn ssh_transport_not_failed_when_the_remote_answered() {
        let answered = CommandResult {
            stdout: b"navidrome.service enabled\n".to_vec(),
            ..CommandResult::transport_failure("")
        };
        assert!(!answered.ssh_transport_failed());
    }

    /// Every other non-zero exit is the remote command's answer, however
    /// unhelpful — a systemd that exits 1 for a name it does not know is not
    /// a transport failure.
    #[test]
    fn ssh_transport_not_failed_on_a_remote_nonzero_exit() {
        assert!(!CommandResult::from_stderr("Unit not found").ssh_transport_failed());
        assert!(!CommandResult::ok().ssh_transport_failed());
    }

    /// The seam raises it, so the one wording reaches a `run_raw` caller that
    /// never sees a `CommandResult`.
    #[test]
    fn mock_run_raw_reports_a_transport_failure_as_unreachable() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::transport_failure(
            "ssh: connect to host: Connection refused",
        ));

        let err = format!(
            "{:#}",
            mock.run_raw(&["systemctl", "list-unit-files"]).unwrap_err()
        );
        assert!(err.contains("unreachable over ssh"), "{err}");
        assert!(err.contains("Connection refused"), "{err}");
    }

    /// A `Route` built for ansible or the ssh include (`key_path: None`) must
    /// fail at construction — before a caller can ever hold a session that
    /// would panic the first time it tried to use it.
    #[test]
    fn test_new_refuses_a_route_with_no_key_path() {
        let host = test_host();
        let route = crate::services::route::resolve(&host, None);
        assert!(LiveSshSession::new(&route, &host.become_method).is_err());
        assert!(LiveSshSession::first_contact(&route, &host.become_method).is_err());
    }
}
