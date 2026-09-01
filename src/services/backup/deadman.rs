//! A Host-side deadman that guards a backup's quiesce window independently of
//! the driver process (ADR-0063).
//!
//! `RecipeExecutor::backup`'s own restart-on-`Err` is a property of the Rust
//! process staying alive to run it. Two real outages happened because the
//! process itself died mid-window — laptop suspend, a Ctrl-C that exits
//! straight to `std::process::exit` — cases no in-process error handling can
//! reach. This module arms a `systemd-run --on-active` timer on the target
//! Host itself before `RecipeExecutor::backup` quiesces an App's units, and
//! re-arms it at every step boundary; if the driver never disarms it in time,
//! the timer fires on the Host alone and brings the units back.

use crate::services::progress::Progress;
use crate::services::ssh::SshSession;
use eyre::Result;
use std::time::Duration;

/// How long any single guarded step may run before the driver is presumed
/// dead. One fixed interval for every App and every step — no per-step timing
/// is recorded anywhere in this codebase to size it against; deliberately
/// generous rather than tuned.
pub const TIMEOUT: Duration = Duration::from_secs(3600);

/// Where a fired deadman's record lives on the Host, for the next
/// `auberge backup` run to find. A file, not driver memory, because the
/// process that armed the timer is exactly what a fire means is gone.
const MARKER_DIR: &str = "/var/lib/auberge/deadman";

/// The transient unit name a deadman arms for `app`. Fixed per App — re-arming
/// replaces the previous timer under the same name rather than stacking a
/// second one alongside it.
fn unit(app: &str) -> String {
    format!("auberge-deadman-{app}")
}

fn marker(app: &str) -> String {
    format!("{MARKER_DIR}/{app}.fired")
}

/// Cancels `app`'s armed deadman, if any. Idempotent: a unit that was never
/// armed, or already fired and exited, is left exactly as absent as it
/// already was — `2>/dev/null` and the trailing `true` are what make a
/// disarm-with-nothing-to-disarm a success rather than a noisy no-op.
pub(crate) fn disarm_command(app: &str) -> String {
    let unit = unit(app);
    format!(
        "sudo systemctl stop {unit}.timer {unit}.service 2>/dev/null; \
         sudo systemctl reset-failed {unit}.timer {unit}.service 2>/dev/null; \
         true"
    )
}

/// Arms a deadman for `app`: after [`TIMEOUT`], runs `reset-failed` then
/// `start` against each of `units` in turn, in the order given — the Backup
/// Recipe's own declared quiesce order (ADR-0032), so a fire replays the same
/// order the executor's own restart already uses. `reset-failed` first
/// because a unit stopped by `SIGTERM` and exiting nonzero latches `failed`,
/// which `start` alone does not clear. Never `restart` or `stop`: `start` on
/// a unit the driver already brought back up is a no-op, which is what makes
/// a fire racing a still-alive driver safe rather than a collision.
///
/// Always disarms first — re-arming under the same unit name requires the
/// previous instance gone, or `systemd-run` refuses with "Unit already
/// exists".
fn arm_command(app: &str, units: &[String]) -> String {
    let unit = unit(app);
    let marker = marker(app);
    let recovery = units
        .iter()
        .map(|u| format!("systemctl reset-failed {u}; systemctl start {u}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "{disarm}; sudo systemd-run --on-active={secs} --unit={unit} /bin/sh -c \
         '{recovery}; mkdir -p {MARKER_DIR}; date -u > {marker}'",
        disarm = disarm_command(app),
        secs = TIMEOUT.as_secs(),
    )
}

/// The check-and-clear run against `app`'s marker: a `cat` that only reaches
/// `rm -f` when the marker exists, so an absent marker leaves nothing to
/// clean up and a present one is cleared the moment it is read — a fire is
/// reported exactly once, by whichever run next asks.
fn fire_check_command(app: &str) -> String {
    let marker = marker(app);
    format!("cat {marker} 2>/dev/null && rm -f {marker}")
}

/// Arms a deadman for `app` over `units`, in declared quiesce order. Launched
/// detached ([`SshSession::run_detached`]): the timer must be scheduled and
/// left running on the Host regardless of what the driver does next,
/// including a slow or hung ssh round trip.
pub fn arm<S: SshSession + ?Sized>(session: &S, app: &str, units: &[String]) -> Result<()> {
    session.run_detached(&arm_command(app, units))
}

/// Cancels `app`'s armed deadman. Best-effort: a disarm that fails to reach
/// the Host leaves a fire path that is already fail-safe (start-only) rather
/// than a backup that fails because its own cleanup step could not confirm
/// itself.
pub fn disarm<S: SshSession + ?Sized>(session: &S, app: &str) {
    let _ = session.run(&disarm_command(app));
}

/// Reads and clears `app`'s fire marker, warning through `progress` when one
/// is found. Evidence that a prior run's driver died mid-quiesce and this
/// Host, not that process, brought the units back — the warning is the only
/// account of it, so the run itself still completes (ADR-0063).
pub fn check_and_report<S: SshSession + ?Sized>(
    session: &S,
    app: &str,
    progress: &mut dyn Progress,
) -> Result<()> {
    let result = session.run(&fire_check_command(app))?;
    let recorded = result.stdout_str();
    let recorded = recorded.trim();
    if !result.success || recorded.is_empty() {
        return Ok(());
    }
    progress.warn(&format!(
        "{app}: a previous backup's host-side deadman fired ({recorded}) — its driver died \
         mid-quiesce and this Host restarted {app}'s units on its own; treat that run's backup \
         as possibly incomplete"
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::progress::{MockProgress, ProgressEvent};
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};

    #[test]
    fn disarm_command_stops_and_clears_both_transient_units_and_still_succeeds_if_absent() {
        let cmd = disarm_command("paperless");
        assert!(cmd.contains(
            "systemctl stop auberge-deadman-paperless.timer auberge-deadman-paperless.service"
        ));
        assert!(cmd.contains("systemctl reset-failed auberge-deadman-paperless.timer auberge-deadman-paperless.service"));
        assert!(cmd.trim_end().ends_with("true"), "{cmd}");
    }

    #[test]
    fn arm_command_disarms_before_arming_under_the_same_unit_name() {
        let cmd = arm_command("paperless", &["paperless-webserver".to_string()]);
        let disarm_pos = cmd
            .find("systemctl stop auberge-deadman-paperless.timer")
            .unwrap();
        let arm_pos = cmd.find("systemd-run").unwrap();
        assert!(disarm_pos < arm_pos, "{cmd}");
    }

    #[test]
    fn arm_command_carries_the_fixed_timeout_and_a_dedicated_unit_name() {
        let cmd = arm_command("bichon", &["bichon".to_string()]);
        assert!(cmd.contains("--on-active=3600"), "{cmd}");
        assert!(cmd.contains("--unit=auberge-deadman-bichon"), "{cmd}");
    }

    #[test]
    fn arm_command_replays_units_in_the_given_order_reset_failed_then_start() {
        let cmd = arm_command(
            "bichon",
            &["bichon-archive.timer".to_string(), "bichon".to_string()],
        );
        let recovery_start = cmd.find("/bin/sh -c '").unwrap() + "/bin/sh -c '".len();
        let recovery = &cmd[recovery_start..];
        assert_eq!(
            recovery
                .split(';')
                .map(str::trim)
                .take(4)
                .collect::<Vec<_>>(),
            vec![
                "systemctl reset-failed bichon-archive.timer",
                "systemctl start bichon-archive.timer",
                "systemctl reset-failed bichon",
                "systemctl start bichon",
            ]
        );
    }

    #[test]
    fn arm_command_never_says_restart_or_stop_against_the_guarded_units() {
        let cmd = arm_command("paperless", &["paperless-webserver".to_string()]);
        let recovery_start = cmd.find("/bin/sh -c '").unwrap();
        let recovery = &cmd[recovery_start..];
        assert!(!recovery.contains("restart"), "{cmd}");
        assert!(!recovery.contains("systemctl stop"), "{cmd}");
    }

    #[test]
    fn arm_command_writes_a_marker_the_next_run_can_read() {
        let cmd = arm_command("paperless", &["paperless-webserver".to_string()]);
        assert!(cmd.contains("mkdir -p /var/lib/auberge/deadman"), "{cmd}");
        assert!(
            cmd.contains("date -u > /var/lib/auberge/deadman/paperless.fired"),
            "{cmd}"
        );
    }

    #[test]
    fn fire_check_command_only_removes_the_marker_when_it_was_read() {
        let cmd = fire_check_command("paperless");
        assert_eq!(
            cmd,
            "cat /var/lib/auberge/deadman/paperless.fired 2>/dev/null && rm -f \
             /var/lib/auberge/deadman/paperless.fired"
        );
    }

    #[test]
    fn arm_sends_the_arm_command_detached() {
        let mock = MockSshSession::new();
        arm(&mock, "paperless", &["paperless-webserver".to_string()]).unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], SshOp::RunDetached(cmd) if cmd.contains("systemd-run")));
    }

    #[test]
    fn disarm_sends_a_blocking_disarm_command() {
        let mock = MockSshSession::new();
        disarm(&mock, "paperless");

        assert_eq!(mock.calls(), vec![SshOp::Run(disarm_command("paperless"))]);
    }

    #[test]
    fn disarm_swallows_a_failing_result_rather_than_panicking() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"Unit not loaded".to_vec(),
        });
        disarm(&mock, "paperless");
    }

    #[test]
    fn check_and_report_warns_and_clears_when_a_marker_is_present() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout("Mon Sep  1 12:00:00 UTC 2026\n"));
        let mut progress = MockProgress::new();

        check_and_report(&mock, "paperless", &mut progress).unwrap();

        let warnings: Vec<String> = progress
            .events()
            .into_iter()
            .filter_map(|e| match e {
                ProgressEvent::Warn(msg) => Some(msg),
                _ => None,
            })
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("paperless"), "{warnings:?}");
        assert!(
            warnings[0].contains("Mon Sep  1 12:00:00 UTC 2026"),
            "{warnings:?}"
        );
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(fire_check_command("paperless"))]
        );
    }

    /// The Mock's own default for an unstaged `run()` call is `success: true`
    /// with empty stdout — nothing like a real "no marker" answer (`cat`
    /// failing makes the whole `&&` chain report failure), but every other
    /// test that reaches a guarded `backup()` without staging anything for
    /// this specific call relies on it not being read as a fire.
    #[test]
    fn check_and_report_ignores_a_successful_but_empty_answer() {
        let mock = MockSshSession::new();
        let mut progress = MockProgress::new();

        check_and_report(&mock, "paperless", &mut progress).unwrap();

        assert!(progress.events().is_empty());
    }

    #[test]
    fn check_and_report_is_silent_when_no_marker_exists() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
        let mut progress = MockProgress::new();

        check_and_report(&mock, "paperless", &mut progress).unwrap();

        assert!(
            progress
                .events()
                .iter()
                .all(|e| !matches!(e, ProgressEvent::Warn(_)))
        );
    }
}
