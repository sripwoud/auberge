use crate::output;
use crate::playbook_meta::FirstDeploySetup;
use crate::ssh_session::SshSession;
use eyre::{Result, WrapErr};
use std::io::IsTerminal;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const POLL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub struct TunnelHandle {
    child: Option<Child>,
}

impl TunnelHandle {
    pub fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn marker_exists(ssh: &SshSession<'_>, marker_path: &str) -> Result<bool> {
    let out = ssh
        .run_raw(&["test", "-e", marker_path])
        .wrap_err_with(|| format!("Failed to check for bootstrap marker at {marker_path}"))?;
    Ok(out.status.success())
}

pub fn spawn_local_forward(ssh: &SshSession<'_>, port: u16) -> Result<TunnelHandle> {
    let forward = format!("{port}:127.0.0.1:{port}");
    let mut cmd = Command::new("ssh");
    cmd.args(ssh.ssh_args());
    cmd.args(["-N", "-L", &forward]);
    let child = cmd
        .spawn()
        .wrap_err_with(|| format!("Failed to spawn SSH local forward on port {port}"))?;
    thread::sleep(Duration::from_millis(500));
    Ok(TunnelHandle::new(child))
}

pub fn wait_for_marker<F>(check: F, interval: Duration, timeout: Duration) -> Result<()>
where
    F: Fn() -> Result<bool>,
{
    let start = Instant::now();
    loop {
        if check()? {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            eyre::bail!(
                "Timed out waiting for Bootstrap Marker after {} seconds",
                timeout.as_secs()
            );
        }
        thread::sleep(interval);
    }
}

pub fn run_bootstrap(ssh: &SshSession<'_>, setup: &FirstDeploySetup) -> Result<()> {
    if marker_exists(ssh, &setup.marker_path)? {
        return Ok(());
    }

    let url = format!("http://localhost:{}{}", setup.port, setup.setup_url_path);

    if !std::io::stderr().is_terminal() {
        print_manual_instructions(ssh, setup, &url);
        eyre::bail!(
            "{} requires interactive setup; rerun in a terminal or follow the manual instructions above",
            setup.wizard_name
        );
    }

    output::info(&format!("{} requires one-time setup.", setup.wizard_name));
    output::info(&format!(
        "Opening SSH tunnel to {}@{}:{} and launching browser at {}",
        ssh.host.user, ssh.host.address, setup.port, url
    ));

    let _tunnel = spawn_local_forward(ssh, setup.port)?;

    if let Err(e) = open::that(&url) {
        output::warn(&format!(
            "Could not open browser automatically ({e}). Open this URL manually: {url}"
        ));
    }

    output::info(&format!(
        "Waiting for {} to complete (polling {} every {}s, Ctrl+C to abort)",
        setup.wizard_name,
        setup.marker_path,
        POLL_INTERVAL.as_secs()
    ));

    wait_for_marker(
        || marker_exists(ssh, &setup.marker_path),
        POLL_INTERVAL,
        POLL_TIMEOUT,
    )?;

    output::success(&format!("{} completed.", setup.wizard_name));
    Ok(())
}

fn print_manual_instructions(ssh: &SshSession<'_>, setup: &FirstDeploySetup, url: &str) {
    eprintln!();
    output::warn(&format!(
        "{} requires one-time interactive setup (non-interactive terminal detected)",
        setup.wizard_name
    ));
    eprintln!(
        "  1. From your laptop, open an SSH tunnel:\n     ssh -L {port}:127.0.0.1:{port} {user}@{addr}",
        port = setup.port,
        user = ssh.host.user,
        addr = ssh.host.address,
    );
    eprintln!("  2. In a browser, open:\n     {url}");
    eprintln!("  3. Complete the wizard, then re-run `auberge deploy` so Caddy + DNS land.",);
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn wait_for_marker_returns_immediately_when_already_present() {
        let result = wait_for_marker(
            || Ok(true),
            Duration::from_millis(1),
            Duration::from_millis(100),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn wait_for_marker_polls_until_present() {
        let attempts = Cell::new(0);
        let result = wait_for_marker(
            || {
                let n = attempts.get();
                attempts.set(n + 1);
                Ok(n >= 3)
            },
            Duration::from_millis(1),
            Duration::from_millis(500),
        );
        assert!(result.is_ok());
        assert_eq!(attempts.get(), 4);
    }

    #[test]
    fn wait_for_marker_times_out() {
        let result = wait_for_marker(
            || Ok(false),
            Duration::from_millis(5),
            Duration::from_millis(20),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Timed out"));
    }

    #[test]
    fn wait_for_marker_propagates_check_error() {
        let result = wait_for_marker(
            || eyre::bail!("SSH connection refused"),
            Duration::from_millis(1),
            Duration::from_millis(100),
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("SSH connection refused"));
    }

    #[test]
    fn tunnel_handle_kills_child_on_drop() {
        let child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        {
            let _h = TunnelHandle::new(child);
        }
        thread::sleep(Duration::from_millis(50));
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .expect("kill -0");
        assert!(
            !status.success(),
            "child process should be dead after TunnelHandle drop"
        );
    }
}
