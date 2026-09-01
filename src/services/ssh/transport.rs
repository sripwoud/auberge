//! Every ssh and scp argv the CLI issues, and the processes that run them.
//!
//! Private to [`super`] on purpose. This type used to be `pub` and named
//! `SshSession` — the same name as the trait one module up, which CONTEXT.md
//! calls the Recipe Executor's only test seam — and four commands imported it
//! past that trait, which is exactly how they became unmockable. The trait is
//! now the only way to reach a Host, and Rust enforces it: nothing outside
//! `services::ssh` can name this type (#669).
//!
//! Mux hygiene is deliberately left as it is: the control socket is never torn
//! down, and `ControlPath` is a fixed `/tmp/ssh-%r@%h:%p`. `ControlPersist`
//! expires the socket on its own, which covers the teardown. The path is the
//! sharper edge — ssh_config(5) recommends a directory "not writable by other
//! users", and `%r` is the *remote* username, so two local users reaching the
//! same remote account collide on one name. Left because this CLI is
//! single-operator by construction (`~/.ssh/identities/<host>/<user>` keys,
//! a user-local `hosts.toml`), and narrowing it is a change to every session's
//! socket path rather than to this refactor. `Reach::FirstContact` opts out
//! entirely where the sharing would actually cost something.

use crate::output;
use crate::services::route::Route;
use eyre::{Context, Result};
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const SSH_MUX_OPTIONS: &[(&str, &str)] = &[
    ("ControlMaster", "auto"),
    ("ControlPath", "/tmp/ssh-%r@%h:%p"),
    ("ControlPersist", "60s"),
];

/// Whether a connection may share a multiplexed master, and whether it keeps
/// the operator's terminal. One choice rather than two flags, because both
/// settings move together and for the same reason: whether trust with the Host
/// is already established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Reuse a warm control socket, capture output. Every command that runs
    /// against a Host the operator already manages.
    Shared,
    /// No multiplexing, and stdin left attached to the terminal.
    ///
    /// `ssh add-key` authorizes a key over a connection made with a key the
    /// operator *named*, and a reused master authenticates with whatever key
    /// opened it — silently discarding that choice. It is also the CLI's one
    /// first-contact command: its target comes from `ansible/inventory.yml`, so
    /// no `hosts.toml` alias block supplies `StrictHostKeyChecking accept-new`,
    /// and ssh may still need to ask about an unknown host key, or for a
    /// passphrase no agent holds. A captured run answers neither.
    FirstContact,
}

pub struct SshTransport<'a> {
    route: &'a Route,
    become_method: &'a str,
    reach: Reach,
}

impl<'a> SshTransport<'a> {
    pub fn new(route: &'a Route, become_method: &'a str) -> Result<Self> {
        Self::require_key(route)?;
        Ok(Self {
            route,
            become_method,
            reach: Reach::Shared,
        })
    }

    pub fn first_contact(route: &'a Route, become_method: &'a str) -> Result<Self> {
        Self::require_key(route)?;
        Ok(Self {
            route,
            become_method,
            reach: Reach::FirstContact,
        })
    }

    /// Every caller resolves a real key before building a `Route` for the ssh
    /// transport specifically (unlike ansible's or the ssh include's Route
    /// use, which pass `None`), so failing here — at construction, rather
    /// than the first time `ssh_key` is later read — turns "this module was
    /// handed the wrong kind of Route" into an error at the mistake's own
    /// call site instead of a panic somewhere downstream of it.
    fn require_key(route: &Route) -> Result<()> {
        if route.key_path.is_none() {
            eyre::bail!(
                "SshTransport requires a Route with a resolved key path (got one built for \
                 ansible or the ssh include, not ssh)"
            );
        }
        Ok(())
    }

    /// The identity file to connect with. `require_key` already checked this
    /// at construction, so the `expect` here can only fire on a bug in that
    /// check, not on a route this module was actually handed.
    fn ssh_key(&self) -> &Path {
        self.route
            .key_path
            .as_deref()
            .expect("checked by require_key at construction")
    }

    /// The connection-sharing options this reach wants. `ControlPath=none` is
    /// spelled out rather than omitted: a user's own `~/.ssh/config` may set a
    /// ControlPath, and `ControlMaster no` — the default — still *joins* an
    /// existing socket.
    fn sharing_args(&self) -> Vec<OsString> {
        match self.reach {
            Reach::Shared => Self::mux_args(),
            Reach::FirstContact => vec!["-o".into(), "ControlPath=none".into()],
        }
    }

    pub fn mux_args() -> Vec<OsString> {
        SSH_MUX_OPTIONS
            .iter()
            .flat_map(|(k, v)| [OsString::from("-o"), format!("{}={}", k, v).into()])
            .collect()
    }

    pub fn ssh_args(&self) -> Vec<OsString> {
        let mut args = self.sharing_args();
        args.extend(self.host_key_alias_args());
        args.extend([
            "-i".into(),
            self.ssh_key().into(),
            "-p".into(),
            self.route.port.to_string().into(),
            format!("{}@{}", self.route.user, self.route.address).into(),
        ]);
        args
    }

    /// `-o HostKeyAlias=<alias>` — every connection checks and saves the host
    /// key under the Host's alias (#785), so a route change (tailnet vs
    /// public address) can never present as a changed key.
    fn host_key_alias_args(&self) -> Vec<OsString> {
        vec![
            "-o".into(),
            format!("HostKeyAlias={}", self.route.alias).into(),
        ]
    }

    /// argv for a bounded, non-interactive connect: the session's own options
    /// plus `ConnectTimeout` and `BatchMode`. Carries the mux options, so a
    /// successful probe leaves a warm control socket for whatever runs next.
    pub fn probe_args(&self, timeout: Duration) -> Vec<OsString> {
        let mut args = self.ssh_args();
        for option in [
            format!("ConnectTimeout={}", timeout.as_secs()),
            "BatchMode=yes".to_string(),
        ] {
            args.extend([OsString::from("-o"), option.into()]);
        }
        args
    }

    pub fn probe(&self, timeout: Duration) -> Result<Output> {
        let out = Command::new("ssh")
            .args(self.probe_args(timeout))
            .arg("true")
            .output()
            .wrap_err("Failed to execute ssh")?;
        let lines = output::subprocess_output("ssh", &String::from_utf8_lossy(&out.stderr));
        if out.status.success() {
            output::clear_subprocess_lines(lines);
        }
        Ok(out)
    }

    pub fn run(&self, command: &str) -> Result<Output> {
        if self.reach == Reach::FirstContact {
            return self.run_attached(command);
        }
        let out = Command::new("ssh")
            .args(self.ssh_args())
            .arg(command)
            .output()
            .wrap_err("Failed to execute SSH command")?;
        let stderr_text = String::from_utf8_lossy(&out.stderr);
        let lines = output::subprocess_output("ssh", &stderr_text);
        if out.status.success() {
            output::clear_subprocess_lines(lines);
        }
        Ok(out)
    }

    /// Streamed, with stdin inherited, so ssh can still prompt. `run_piped`
    /// keeps only the stderr tail, which is what the caller's error reports;
    /// stdout is not captured, and no [`Reach::FirstContact`] caller reads it.
    fn run_attached(&self, command: &str) -> Result<Output> {
        let result = output::run_piped(
            "ssh",
            Command::new("ssh").args(self.ssh_args()).arg(command),
        )
        .wrap_err("Failed to execute SSH command")?;
        if result.status.success() {
            output::clear_subprocess_lines(result.lines_written);
        }
        Ok(Output {
            status: result.status,
            stdout: Vec::new(),
            stderr: result.last_stderr.into_bytes(),
        })
    }

    /// Launches `command` over ssh without waiting for it to exit — every
    /// other method here blocks until the remote command's own exit, which is
    /// wrong for arming a Host-side deadman (ADR-0066): the timer must be
    /// scheduled and left running on the Host regardless of what happens to
    /// the driver next, including a slow or hung ssh round trip blocking the
    /// backup itself. The child is left unwaited on purpose; this process is
    /// short-lived and any orphan is reparented and reaped by init.
    pub fn spawn(&self, command: &str) -> Result<()> {
        Command::new("ssh")
            .args(self.ssh_args())
            .arg(command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("Failed to launch detached SSH command")?;
        Ok(())
    }

    pub fn run_raw(&self, args: &[&str]) -> Result<Output> {
        let mut cmd = Command::new("ssh");
        cmd.args(self.ssh_args());
        for arg in args {
            cmd.arg(arg);
        }
        let out = cmd.output().wrap_err("Failed to execute SSH command")?;
        let stderr_text = String::from_utf8_lossy(&out.stderr);
        let lines = output::subprocess_output("ssh", &stderr_text);
        if out.status.success() {
            output::clear_subprocess_lines(lines);
        }
        Ok(out)
    }

    pub fn rsync_e_arg(&self) -> String {
        let mux = SSH_MUX_OPTIONS
            .iter()
            .map(|(k, v)| format!("-o {}={}", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        let key = shell_escape::escape(self.ssh_key().display().to_string().into());
        let alias = shell_escape::escape(self.route.alias.clone().into());
        format!(
            "ssh {} -o HostKeyAlias={} -i {} -p {}",
            mux, alias, key, self.route.port
        )
    }

    pub fn scp_args(&self) -> Vec<OsString> {
        let mut args = self.sharing_args();
        args.extend(self.host_key_alias_args());
        args.extend([
            "-i".into(),
            self.ssh_key().into(),
            "-P".into(),
            self.route.port.to_string().into(),
        ]);
        args
    }

    pub fn scp_to(&self, local: &Path, remote: &str) -> Result<()> {
        let out = Command::new("scp")
            .args(self.scp_args())
            .arg(local)
            .arg(format!(
                "{}@{}:{}",
                self.route.user, self.route.address, remote
            ))
            .output()
            .wrap_err("Failed to upload file via scp")?;
        let stderr_text = String::from_utf8_lossy(&out.stderr);
        let lines = output::subprocess_output("scp", &stderr_text);
        if out.status.success() {
            output::clear_subprocess_lines(lines);
        }
        if !out.status.success() {
            let stderr = stderr_text.trim();
            if stderr.is_empty() {
                eyre::bail!("scp to {}:{} failed", self.route.address, remote);
            } else {
                eyre::bail!(
                    "scp to {}:{} failed: {}",
                    self.route.address,
                    remote,
                    stderr
                );
            }
        }
        Ok(())
    }

    pub fn scp_from(&self, remote: &str, local: &Path) -> Result<()> {
        let out = Command::new("scp")
            .args(self.scp_args())
            .arg(format!(
                "{}@{}:{}",
                self.route.user, self.route.address, remote
            ))
            .arg(local)
            .output()
            .wrap_err("Failed to download file via scp")?;
        let stderr_text = String::from_utf8_lossy(&out.stderr);
        let lines = output::subprocess_output("scp", &stderr_text);
        if out.status.success() {
            output::clear_subprocess_lines(lines);
        }
        if !out.status.success() {
            let stderr = stderr_text.trim();
            if stderr.is_empty() {
                eyre::bail!("scp from {}:{} failed", self.route.address, remote);
            } else {
                eyre::bail!(
                    "scp from {}:{} failed: {}",
                    self.route.address,
                    remote,
                    stderr
                );
            }
        }
        Ok(())
    }

    /// The remote argv systemctl runs, escalated with the acting Host's
    /// `become_method` (`sudo` by default, see #776).
    fn systemctl_remote_args<'b>(&'b self, action: &'b str, service: &'b str) -> Vec<&'b str> {
        vec![self.become_method, "systemctl", action, service]
    }

    pub fn systemctl(&self, action: &str, service: &str) -> Result<()> {
        let result = output::run_piped(
            "systemctl",
            Command::new("ssh")
                .args(self.ssh_args())
                .args(self.systemctl_remote_args(action, service)),
        )
        .wrap_err_with(|| format!("Failed to {} service {}", action, service))?;
        if result.status.success() {
            output::clear_subprocess_lines(result.lines_written);
        }
        if !result.status.success() {
            return Err(result.error(format!("systemctl {} {} failed", action, service)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_route() -> Route {
        Route {
            address: "192.0.2.1".to_string(),
            port: 2222,
            user: "deploy".to_string(),
            key_path: Some(PathBuf::from("/home/user/.ssh/id_ed25519")),
            alias: "test".to_string(),
        }
    }

    const BECOME_METHOD: &str = "sudo";

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn test_ssh_args_contains_mux_options() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.ssh_args());
        assert!(strs.contains(&"ControlMaster=auto".to_string()));
        assert!(strs.contains(&"ControlPath=/tmp/ssh-%r@%h:%p".to_string()));
        assert!(strs.contains(&"ControlPersist=60s".to_string()));
    }

    #[test]
    fn test_ssh_args_includes_key_port_user_host() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.ssh_args());
        assert!(strs.contains(&"/home/user/.ssh/id_ed25519".to_string()));
        assert!(strs.contains(&"2222".to_string()));
        assert!(strs.contains(&"deploy@192.0.2.1".to_string()));
    }

    #[test]
    fn test_ssh_args_pins_the_host_key_lookup_to_the_hosts_alias() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.ssh_args());
        assert!(strs.contains(&"HostKeyAlias=test".to_string()), "{strs:?}");
    }

    #[test]
    fn test_first_contact_also_pins_the_host_key_alias() {
        let route = Route {
            key_path: Some(PathBuf::from("/tmp/chosen_key")),
            ..test_route()
        };
        let strs = strings(
            &SshTransport::first_contact(&route, BECOME_METHOD)
                .unwrap()
                .ssh_args(),
        );
        assert!(strs.contains(&"HostKeyAlias=test".to_string()), "{strs:?}");
    }

    #[test]
    fn test_scp_args_pins_the_host_key_lookup_to_the_hosts_alias() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.scp_args());
        assert!(strs.contains(&"HostKeyAlias=test".to_string()), "{strs:?}");
    }

    #[test]
    fn test_rsync_e_arg_pins_the_host_key_lookup_to_the_hosts_alias() {
        let route = test_route();
        let e_arg = SshTransport::new(&route, BECOME_METHOD)
            .unwrap()
            .rsync_e_arg();
        assert!(e_arg.contains("HostKeyAlias=test"), "{e_arg}");
    }

    #[test]
    fn test_scp_args_uses_uppercase_p_for_port() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.scp_args());
        assert!(strs.contains(&"-P".to_string()));
        assert!(!strs.contains(&"-p".to_string()));
    }

    #[test]
    fn test_rsync_e_arg_contains_mux_and_key() {
        let route = test_route();
        let e_arg = SshTransport::new(&route, BECOME_METHOD)
            .unwrap()
            .rsync_e_arg();
        assert!(e_arg.starts_with("ssh "));
        assert!(e_arg.contains("ControlMaster=auto"));
        assert!(e_arg.contains("ControlPath=/tmp/ssh-%r@%h:%p"));
        assert!(e_arg.contains("ControlPersist=60s"));
        assert!(e_arg.contains("-i /home/user/.ssh/id_ed25519"));
        assert!(e_arg.contains("-p 2222"));
    }

    #[test]
    fn test_rsync_e_arg_escapes_spaces_in_key_path() {
        let route = Route {
            key_path: Some(PathBuf::from("/home/user/my keys/id_ed25519")),
            ..test_route()
        };
        let e_arg = SshTransport::new(&route, BECOME_METHOD)
            .unwrap()
            .rsync_e_arg();
        assert!(!e_arg.contains("-i /home/user/my keys/id_ed25519"));
        assert!(e_arg.contains("'/home/user/my keys/id_ed25519'"));
    }

    #[test]
    fn test_probe_args_bound_the_connect_and_forbid_prompts() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.probe_args(Duration::from_secs(10)));
        assert!(strs.contains(&"ConnectTimeout=10".to_string()));
        assert!(strs.contains(&"BatchMode=yes".to_string()));
    }

    #[test]
    fn test_probe_args_keep_the_mux_options_so_the_socket_stays_warm() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        let strs = strings(&session.probe_args(Duration::from_secs(3)));
        assert!(strs.contains(&"ControlMaster=auto".to_string()));
        assert!(strs.contains(&"ControlPersist=60s".to_string()));
        assert!(strs.contains(&"deploy@192.0.2.1".to_string()));
    }

    #[test]
    fn test_first_contact_refuses_to_share_a_master() {
        let route = Route {
            key_path: Some(PathBuf::from("/tmp/chosen_key")),
            ..test_route()
        };
        let strs = strings(
            &SshTransport::first_contact(&route, BECOME_METHOD)
                .unwrap()
                .ssh_args(),
        );
        assert!(strs.contains(&"ControlPath=none".to_string()), "{strs:?}");
        assert!(
            !strs.iter().any(|s| s.starts_with("ControlMaster")),
            "{strs:?}"
        );
        assert!(
            !strs.iter().any(|s| s.starts_with("ControlPersist")),
            "{strs:?}"
        );
    }

    #[test]
    fn test_first_contact_still_names_the_key_the_operator_chose() {
        let route = Route {
            key_path: Some(PathBuf::from("/tmp/chosen_key")),
            ..test_route()
        };
        let strs = strings(
            &SshTransport::first_contact(&route, BECOME_METHOD)
                .unwrap()
                .ssh_args(),
        );
        assert!(strs.contains(&"/tmp/chosen_key".to_string()), "{strs:?}");
        assert!(strs.contains(&"deploy@192.0.2.1".to_string()), "{strs:?}");
    }

    #[test]
    fn test_shared_reach_is_the_default_and_multiplexes() {
        let route = test_route();
        let strs = strings(&SshTransport::new(&route, BECOME_METHOD).unwrap().ssh_args());
        assert!(strs.contains(&"ControlMaster=auto".to_string()), "{strs:?}");
        assert!(!strs.contains(&"ControlPath=none".to_string()), "{strs:?}");
    }

    #[test]
    fn test_first_contact_scp_args_do_not_share_either() {
        let route = test_route();
        let strs = strings(
            &SshTransport::first_contact(&route, BECOME_METHOD)
                .unwrap()
                .scp_args(),
        );
        assert!(strs.contains(&"ControlPath=none".to_string()), "{strs:?}");
    }

    #[test]
    fn test_systemctl_remote_args_defaults_to_sudo() {
        let route = test_route();
        let session = SshTransport::new(&route, "sudo").unwrap();
        assert_eq!(
            session.systemctl_remote_args("restart", "paperless-webserver"),
            vec!["sudo", "systemctl", "restart", "paperless-webserver"]
        );
    }

    #[test]
    fn test_systemctl_remote_args_uses_configured_become_method() {
        let route = test_route();
        let session = SshTransport::new(&route, "doas").unwrap();
        assert_eq!(
            session.systemctl_remote_args("restart", "paperless-webserver"),
            vec!["doas", "systemctl", "restart", "paperless-webserver"]
        );
    }

    #[test]
    fn test_spawn_returns_before_the_remote_command_would_finish() {
        let route = test_route();
        let session = SshTransport::new(&route, BECOME_METHOD).unwrap();
        // Not a live ssh: the far end refuses instantly. What this asserts is
        // that `spawn` itself returns immediately rather than blocking on
        // that refusal — an `output()`-based call would too, here, so the
        // real guarantee is that this is `Command::spawn`, not `.output()`.
        let start = std::time::Instant::now();
        session.spawn("sleep 5").unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_mux_args_pairs_options_correctly() {
        let strs = strings(&SshTransport::mux_args());
        for (i, s) in strs.iter().enumerate() {
            if s == "-o" {
                assert!(
                    strs[i + 1].contains('='),
                    "option after -o should be key=value"
                );
            }
        }
    }

    /// A `Route` built for ansible or the ssh include (`key_path: None`) must
    /// be refused at construction, not accepted and left to panic the first
    /// time `ssh_args`/`scp_args`/`rsync_e_arg` reads a key that isn't there.
    #[test]
    fn test_new_refuses_a_route_with_no_key_path() {
        let route = Route {
            key_path: None,
            ..test_route()
        };
        assert!(SshTransport::new(&route, BECOME_METHOD).is_err());
        assert!(SshTransport::first_contact(&route, BECOME_METHOD).is_err());
    }
}
