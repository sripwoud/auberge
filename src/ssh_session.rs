use crate::hosts::Host;
use crate::output;
use eyre::{Context, Result};
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

const SSH_MUX_OPTIONS: &[(&str, &str)] = &[
    ("ControlMaster", "auto"),
    ("ControlPath", "/tmp/ssh-%r@%h:%p"),
    ("ControlPersist", "60s"),
];

pub struct SshSession<'a> {
    pub host: &'a Host,
    ssh_key: &'a Path,
}

impl<'a> SshSession<'a> {
    pub fn new(host: &'a Host, ssh_key: &'a Path) -> Self {
        Self { host, ssh_key }
    }

    pub fn mux_args() -> Vec<OsString> {
        SSH_MUX_OPTIONS
            .iter()
            .flat_map(|(k, v)| [OsString::from("-o"), format!("{}={}", k, v).into()])
            .collect()
    }

    pub fn ssh_args(&self) -> Vec<OsString> {
        let mut args = Self::mux_args();
        args.extend([
            "-i".into(),
            self.ssh_key.into(),
            "-p".into(),
            self.host.port.to_string().into(),
            format!("{}@{}", self.host.user, self.host.address).into(),
        ]);
        args
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
        Command::new("ssh")
            .args(self.probe_args(timeout))
            .arg("true")
            .output()
            .wrap_err("Failed to execute ssh")
    }

    pub fn run(&self, command: &str) -> Result<Output> {
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
        let key = shell_escape::escape(self.ssh_key.display().to_string().into());
        format!("ssh {} -i {} -p {}", mux, key, self.host.port)
    }

    pub fn scp_args(&self) -> Vec<OsString> {
        let mut args = Self::mux_args();
        args.extend([
            "-i".into(),
            self.ssh_key.into(),
            "-P".into(),
            self.host.port.to_string().into(),
        ]);
        args
    }

    pub fn scp_to(&self, local: &Path, remote: &str) -> Result<()> {
        let out = Command::new("scp")
            .args(self.scp_args())
            .arg(local)
            .arg(format!(
                "{}@{}:{}",
                self.host.user, self.host.address, remote
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
                eyre::bail!("scp to {}:{} failed", self.host.address, remote);
            } else {
                eyre::bail!("scp to {}:{} failed: {}", self.host.address, remote, stderr);
            }
        }
        Ok(())
    }

    pub fn scp_from(&self, remote: &str, local: &Path) -> Result<()> {
        let out = Command::new("scp")
            .args(self.scp_args())
            .arg(format!(
                "{}@{}:{}",
                self.host.user, self.host.address, remote
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
                eyre::bail!("scp from {}:{} failed", self.host.address, remote);
            } else {
                eyre::bail!(
                    "scp from {}:{} failed: {}",
                    self.host.address,
                    remote,
                    stderr
                );
            }
        }
        Ok(())
    }

    pub fn systemctl(&self, action: &str, service: &str) -> Result<()> {
        let result = output::run_piped(
            "systemctl",
            Command::new("ssh")
                .args(self.ssh_args())
                .arg("sudo")
                .arg("systemctl")
                .arg(action)
                .arg(service),
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

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn test_ssh_args_contains_mux_options() {
        let host = test_host();
        let key = Path::new("/home/user/.ssh/id_ed25519");
        let session = SshSession::new(&host, key);
        let strs = strings(&session.ssh_args());
        assert!(strs.contains(&"ControlMaster=auto".to_string()));
        assert!(strs.contains(&"ControlPath=/tmp/ssh-%r@%h:%p".to_string()));
        assert!(strs.contains(&"ControlPersist=60s".to_string()));
    }

    #[test]
    fn test_ssh_args_includes_key_port_user_host() {
        let host = test_host();
        let key = Path::new("/home/user/.ssh/id_ed25519");
        let session = SshSession::new(&host, key);
        let strs = strings(&session.ssh_args());
        assert!(strs.contains(&"/home/user/.ssh/id_ed25519".to_string()));
        assert!(strs.contains(&"2222".to_string()));
        assert!(strs.contains(&"deploy@192.0.2.1".to_string()));
    }

    #[test]
    fn test_scp_args_uses_uppercase_p_for_port() {
        let host = test_host();
        let key = Path::new("/tmp/key");
        let session = SshSession::new(&host, key);
        let strs = strings(&session.scp_args());
        assert!(strs.contains(&"-P".to_string()));
        assert!(!strs.contains(&"-p".to_string()));
    }

    #[test]
    fn test_rsync_e_arg_contains_mux_and_key() {
        let host = test_host();
        let key = Path::new("/home/user/.ssh/id_ed25519");
        let e_arg = SshSession::new(&host, key).rsync_e_arg();
        assert!(e_arg.starts_with("ssh "));
        assert!(e_arg.contains("ControlMaster=auto"));
        assert!(e_arg.contains("ControlPath=/tmp/ssh-%r@%h:%p"));
        assert!(e_arg.contains("ControlPersist=60s"));
        assert!(e_arg.contains("-i /home/user/.ssh/id_ed25519"));
        assert!(e_arg.contains("-p 2222"));
    }

    #[test]
    fn test_rsync_e_arg_escapes_spaces_in_key_path() {
        let host = test_host();
        let key = Path::new("/home/user/my keys/id_ed25519");
        let e_arg = SshSession::new(&host, key).rsync_e_arg();
        assert!(!e_arg.contains("-i /home/user/my keys/id_ed25519"));
        assert!(e_arg.contains("'/home/user/my keys/id_ed25519'"));
    }

    #[test]
    fn test_probe_args_bound_the_connect_and_forbid_prompts() {
        let host = test_host();
        let key = Path::new("/tmp/key");
        let session = SshSession::new(&host, key);
        let strs = strings(&session.probe_args(Duration::from_secs(10)));
        assert!(strs.contains(&"ConnectTimeout=10".to_string()));
        assert!(strs.contains(&"BatchMode=yes".to_string()));
    }

    #[test]
    fn test_probe_args_keep_the_mux_options_so_the_socket_stays_warm() {
        let host = test_host();
        let key = Path::new("/tmp/key");
        let session = SshSession::new(&host, key);
        let strs = strings(&session.probe_args(Duration::from_secs(3)));
        assert!(strs.contains(&"ControlMaster=auto".to_string()));
        assert!(strs.contains(&"ControlPersist=60s".to_string()));
        assert!(strs.contains(&"deploy@192.0.2.1".to_string()));
    }

    #[test]
    fn test_mux_args_pairs_options_correctly() {
        let strs = strings(&SshSession::mux_args());
        for (i, s) in strs.iter().enumerate() {
            if s == "-o" {
                assert!(
                    strs[i + 1].contains('='),
                    "option after -o should be key=value"
                );
            }
        }
    }
}
