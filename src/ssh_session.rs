use crate::hosts::Host;
use crate::output;
use eyre::{Context, Result};
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};

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
            eyre::bail!("systemctl {} {} failed", action, service);
        }
        Ok(())
    }
}
