//! A Host-side deadman that guards a quiesce window independently of the
//! driver process (ADR-0066).
//!
//! `RecipeExecutor`'s own restart-on-`Err` is a property of the Rust process
//! staying alive to run it. Two real outages happened because the process
//! itself died mid-window — laptop suspend, a Ctrl-C that exits straight to
//! `std::process::exit` — cases no in-process error handling can reach. This
//! module arms a `systemd-run --on-active` timer on the target Host itself
//! before an App's units are quiesced, and re-arms it at every step boundary;
//! if the driver never disarms it in time, the timer fires on the Host alone
//! and brings the units back.
//!
//! Both of the executor's quiesce windows are guarded — `backup`'s and
//! `restore`'s. Which one armed a timer is recorded in the fire marker,
//! because an interrupted backup and an interrupted restore leave the App in
//! entirely different states (#775).

use crate::services::progress::Progress;
use crate::services::ssh::SshSession;
use eyre::Result;
use std::time::Duration;

/// How long any single guarded step may run before the driver is presumed
/// dead. One fixed interval for every App and every step — no per-step timing
/// is recorded anywhere in this codebase to size it against; deliberately
/// generous rather than tuned.
pub const TIMEOUT: Duration = Duration::from_secs(3600);

/// Which quiesce window a deadman is guarding. Written into the fire marker
/// so the run that finds it can say what died, not only when: an interrupted
/// backup leaves a snapshot that may be incomplete, an interrupted restore
/// leaves the App itself half-overwritten (#775).
#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Backup,
    Restore,
}

impl Operation {
    /// Every operation a deadman can guard. The one list [`Self::parse`] and
    /// the tests scan, so a new variant is reachable everywhere the moment
    /// [`Self::as_str`] stops compiling without it.
    const ALL: [Self; 2] = [Self::Backup, Self::Restore];

    /// The token this operation writes into its fire marker.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Restore => "restore",
        }
    }

    /// The operation a marker's leading token names, or `None` for one that
    /// names none — every marker written before #775 held a bare timestamp.
    fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.as_str() == token)
    }
}

/// Where a fired deadman's record lives on the Host, for the next guarded run
/// against the same App to find. A file, not driver memory, because the
/// process that armed the timer is exactly what a fire means is gone.
const MARKER_DIR: &str = "/var/lib/auberge/deadman";

/// What every transient unit a deadman arms is named after, whichever App and
/// whichever operation.
const UNIT_PREFIX: &str = "auberge-deadman-";

/// The transient unit name a deadman arms for `app`. Fixed per App — re-arming
/// replaces the previous timer under the same name rather than stacking a
/// second one alongside it.
fn unit(app: &str) -> String {
    format!("{UNIT_PREFIX}{app}")
}

/// The write a fired deadman makes to its marker: one line, the operation
/// leading, so [`check_and_report`] can split it off the front without
/// parsing `date`'s own field count.
fn marker_write(operation: Operation) -> String {
    format!("echo \"{} $(date -u)\"", operation.as_str())
}

fn marker(app: &str) -> String {
    format!("{MARKER_DIR}/{app}.fired")
}

/// Cancels `app`'s armed deadman, if any. Idempotent: a unit that was never
/// armed, or already fired and exited, is left exactly as absent as it
/// already was — `2>/dev/null` and the trailing `true` are what make a
/// disarm-with-nothing-to-disarm a success rather than a noisy no-op.
/// Escalated with the acting Host's `become_method` (`sudo` by default,
/// see #776) — `systemctl` here runs outside `SshSession::systemctl`'s own
/// escalation, since arm/disarm build their own command line.
pub(crate) fn disarm_command(app: &str, become_method: &str) -> String {
    let unit = unit(app);
    format!(
        "{become_method} systemctl stop {unit}.timer {unit}.service 2>/dev/null; \
         {become_method} systemctl reset-failed {unit}.timer {unit}.service 2>/dev/null; \
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
/// exists". `systemd-run` itself is what needs escalating: once it runs as
/// root, the transient unit's own `systemctl`/`mkdir`/`date` inherit that,
/// with no further escalation needed inside it.
fn arm_command(app: &str, operation: Operation, units: &[String], become_method: &str) -> String {
    let unit = unit(app);
    let marker = marker(app);
    let recovery = units
        .iter()
        .map(|u| format!("systemctl reset-failed {u}; systemctl start {u}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "{disarm}; {become_method} systemd-run --on-active={secs} --unit={unit} /bin/sh -c \
         '{recovery}; mkdir -p {MARKER_DIR}; {write} > {marker}'",
        disarm = disarm_command(app, become_method),
        secs = TIMEOUT.as_secs(),
        write = marker_write(operation),
    )
}

/// The check-and-clear run against `app`'s marker: a `cat` that only reaches
/// `rm -f` when the marker exists, so an absent marker leaves nothing to
/// clean up and a present one is cleared the moment it is read — a fire is
/// reported exactly once, by whichever run next asks. `cat` needs no
/// escalation (the marker is world-readable), but `rm -f` does: the marker
/// lives in a directory the arming `systemd-run` created as root, and unlink
/// permission comes from the directory, not the file.
fn fire_check_command(app: &str, become_method: &str) -> String {
    let marker = marker(app);
    format!("cat {marker} 2>/dev/null && {become_method} rm -f {marker}")
}

/// Arms a deadman for `app` over `units`, in declared quiesce order. Launched
/// detached ([`SshSession::run_detached`]): the timer must be scheduled and
/// left running on the Host regardless of what the driver does next,
/// including a slow or hung ssh round trip.
pub fn arm<S: SshSession + ?Sized>(
    session: &S,
    app: &str,
    operation: Operation,
    units: &[String],
) -> Result<()> {
    session.run_detached(&arm_command(app, operation, units, session.become_method()))
}

/// Cancels `app`'s armed deadman. Best-effort: a disarm that fails to reach
/// the Host leaves a fire path that is already fail-safe (start-only) rather
/// than a backup that fails because its own cleanup step could not confirm
/// itself.
pub fn disarm<S: SshSession + ?Sized>(session: &S, app: &str) {
    let _ = session.run(&disarm_command(app, session.become_method()));
}

/// Reads and clears `app`'s fire marker, warning through `progress` when one
/// is found. Evidence that a prior run's driver died mid-quiesce and this
/// Host, not that process, brought the units back — the warning is the only
/// account of it, so the run itself still completes (ADR-0066).
pub fn check_and_report<S: SshSession + ?Sized>(
    session: &S,
    app: &str,
    progress: &mut dyn Progress,
) -> Result<()> {
    let result = session.run(&fire_check_command(app, session.become_method()))?;
    let recorded = result.stdout_str();
    let recorded = recorded.trim();
    if !result.success || recorded.is_empty() {
        return Ok(());
    }
    progress.warn(&fire_warning(app, recorded));
    Ok(())
}

/// The account a fired marker gets: what died, when, and what it leaves the
/// operator holding. The operation is the marker's first field; a marker that
/// does not start with one — a legacy timestamp-only marker written before
/// #775, or anything else unparseable — still warns, because a fire nobody is
/// told about is the one outcome this whole mechanism exists to prevent.
fn fire_warning(app: &str, recorded: &str) -> String {
    let named = recorded
        .split_once(' ')
        .and_then(|(token, when)| Operation::parse(token).map(|op| (op, when)));
    let (subject, when, consequence) = match named {
        Some((op @ Operation::Backup, when)) => (
            op.as_str(),
            when,
            "treat that run's backup as possibly incomplete".to_string(),
        ),
        Some((op @ Operation::Restore, when)) => (
            op.as_str(),
            when,
            format!(
                "that restore was interrupted mid-apply, so {app} may be half-overwritten — \
                 re-run the restore, or roll back from the emergency backup a cross-host \
                 restore takes first"
            ),
        ),
        None => (
            "run",
            recorded,
            format!(
                "the marker does not name the operation that armed it, so treat both the \
                 snapshot it may have been taking and {app}'s own state as suspect"
            ),
        ),
    };
    format!(
        "{app}: a previous {subject}'s host-side deadman fired ({when}) — its driver died \
         mid-quiesce and this Host restarted {app}'s units on its own; {consequence}"
    )
}

/// How a deadman's own SSH calls are picked out of a call log.
///
/// Here, beside the commands that produce them, because three test modules
/// need to tell a deadman's traffic from the App's own: a marker path or unit
/// name changed in one place and re-spelled in three would not fail those
/// tests, it would make them pass over nothing.
#[cfg(test)]
pub mod recognise {
    use super::{MARKER_DIR, Operation, UNIT_PREFIX, marker_write};
    use crate::services::ssh::SshOp;

    #[derive(Debug)]
    pub enum DeadmanOp {
        FireCheck,
        Arm(Operation),
        Disarm,
    }

    /// Which deadman call this is, or `None` for a call belonging to the App's
    /// own stop/transfer/restart sequence.
    pub fn of(call: &SshOp) -> Option<DeadmanOp> {
        match call {
            SshOp::RunDetached(cmd) => Operation::ALL
                .into_iter()
                .find(|op| cmd.contains(&marker_write(*op)))
                .map(DeadmanOp::Arm),
            SshOp::Run(cmd) if cmd.starts_with(&format!("cat {MARKER_DIR}")) => {
                Some(DeadmanOp::FireCheck)
            }
            SshOp::Run(cmd) if cmd.contains(UNIT_PREFIX) => Some(DeadmanOp::Disarm),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::progress::{MockProgress, ProgressEvent};
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};

    #[test]
    fn disarm_command_stops_and_clears_both_transient_units_and_still_succeeds_if_absent() {
        let cmd = disarm_command("paperless", "sudo");
        assert!(cmd.contains(
            "systemctl stop auberge-deadman-paperless.timer auberge-deadman-paperless.service"
        ));
        assert!(cmd.contains("systemctl reset-failed auberge-deadman-paperless.timer auberge-deadman-paperless.service"));
        assert!(cmd.trim_end().ends_with("true"), "{cmd}");
    }

    #[test]
    fn arm_command_disarms_before_arming_under_the_same_unit_name() {
        let cmd = arm_command(
            "paperless",
            Operation::Backup,
            &["paperless-webserver".to_string()],
            "sudo",
        );
        let disarm_pos = cmd
            .find("systemctl stop auberge-deadman-paperless.timer")
            .unwrap();
        let arm_pos = cmd.find("systemd-run").unwrap();
        assert!(disarm_pos < arm_pos, "{cmd}");
    }

    #[test]
    fn arm_command_carries_the_fixed_timeout_and_a_dedicated_unit_name() {
        let cmd = arm_command("bichon", Operation::Backup, &["bichon".to_string()], "sudo");
        assert!(cmd.contains("--on-active=3600"), "{cmd}");
        assert!(cmd.contains("--unit=auberge-deadman-bichon"), "{cmd}");
    }

    #[test]
    fn arm_and_disarm_commands_use_the_hosts_configured_become_method() {
        let arm = arm_command("bichon", Operation::Backup, &["bichon".to_string()], "doas");
        let disarm = disarm_command("bichon", "doas");
        assert!(
            arm.contains("doas systemctl stop auberge-deadman-bichon"),
            "{arm}"
        );
        assert!(arm.contains("doas systemd-run"), "{arm}");
        assert!(!arm.contains("sudo"), "{arm}");
        assert!(disarm.contains("doas systemctl"), "{disarm}");
        assert!(!disarm.contains("sudo"), "{disarm}");
    }

    #[test]
    fn fire_check_command_escalates_the_removal_with_the_hosts_become_method() {
        let cmd = fire_check_command("paperless", "doas");
        assert!(cmd.contains("&& doas rm -f"), "{cmd}");
        assert!(!cmd.contains("sudo"), "{cmd}");
    }

    #[test]
    fn arm_command_replays_units_in_the_given_order_reset_failed_then_start() {
        let cmd = arm_command(
            "bichon",
            Operation::Backup,
            &["bichon-archive.timer".to_string(), "bichon".to_string()],
            "sudo",
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
        let cmd = arm_command(
            "paperless",
            Operation::Backup,
            &["paperless-webserver".to_string()],
            "sudo",
        );
        let recovery_start = cmd.find("/bin/sh -c '").unwrap();
        let recovery = &cmd[recovery_start..];
        assert!(!recovery.contains("restart"), "{cmd}");
        assert!(!recovery.contains("systemctl stop"), "{cmd}");
    }

    #[test]
    fn arm_command_writes_a_marker_naming_the_operation_and_the_time() {
        let cmd = arm_command(
            "paperless",
            Operation::Backup,
            &["paperless-webserver".to_string()],
            "sudo",
        );
        assert!(cmd.contains("mkdir -p /var/lib/auberge/deadman"), "{cmd}");
        assert!(
            cmd.contains("echo \"backup $(date -u)\" > /var/lib/auberge/deadman/paperless.fired"),
            "{cmd}"
        );
    }

    /// The whole point of the payload: the same App, the same unit name, the
    /// same marker path — only the operation separates a fired backup from a
    /// fired restore.
    #[test]
    fn arm_command_records_restore_as_the_operation_when_restore_arms_it() {
        let cmd = arm_command(
            "paperless",
            Operation::Restore,
            &["paperless-webserver".to_string()],
            "sudo",
        );
        assert!(
            cmd.contains("echo \"restore $(date -u)\" > /var/lib/auberge/deadman/paperless.fired"),
            "{cmd}"
        );
    }

    #[test]
    fn fire_check_command_only_removes_the_marker_when_it_was_read() {
        let cmd = fire_check_command("paperless", "sudo");
        assert_eq!(
            cmd,
            "cat /var/lib/auberge/deadman/paperless.fired 2>/dev/null && sudo rm -f \
             /var/lib/auberge/deadman/paperless.fired"
        );
    }

    #[test]
    fn arm_sends_the_arm_command_detached() {
        let mock = MockSshSession::new();
        arm(
            &mock,
            "paperless",
            Operation::Backup,
            &["paperless-webserver".to_string()],
        )
        .unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], SshOp::RunDetached(cmd) if cmd.contains("systemd-run")));
    }

    #[test]
    fn arm_escalates_with_the_sessions_configured_become_method() {
        let mock = MockSshSession::with_become_method("doas");
        arm(
            &mock,
            "paperless",
            Operation::Backup,
            &["paperless-webserver".to_string()],
        )
        .unwrap();

        let calls = mock.calls();
        assert!(
            matches!(&calls[0], SshOp::RunDetached(cmd) if cmd.contains("doas systemd-run") && !cmd.contains("sudo")),
            "{calls:?}"
        );
    }

    #[test]
    fn disarm_sends_a_blocking_disarm_command() {
        let mock = MockSshSession::new();
        disarm(&mock, "paperless");

        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(disarm_command("paperless", "sudo"))]
        );
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
        mock.stage_run_result(CommandResult::from_stdout(
            "backup Mon Sep  1 12:00:00 UTC 2026\n",
        ));
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
            vec![SshOp::Run(fire_check_command("paperless", "sudo"))]
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

    fn warning_for(marker: &str) -> String {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(marker));
        let mut progress = MockProgress::new();

        check_and_report(&mock, "paperless", &mut progress).unwrap();

        let mut warnings = progress.events().into_iter().filter_map(|e| match e {
            ProgressEvent::Warn(msg) => Some(msg),
            _ => None,
        });
        let warning = warnings.next().expect("a fire is always reported");
        assert!(warnings.next().is_none(), "a fire is reported exactly once");
        warning
    }

    #[test]
    fn a_fired_backup_marker_says_the_snapshot_may_be_incomplete() {
        let warning = warning_for("backup Mon Sep  1 12:00:00 UTC 2026\n");
        assert!(
            warning.contains("a previous backup's host-side deadman fired"),
            "{warning}"
        );
        assert!(
            warning.contains("Mon Sep  1 12:00:00 UTC 2026"),
            "{warning}"
        );
        assert!(
            warning.contains("treat that run's backup as possibly incomplete"),
            "{warning}"
        );
    }

    /// A restore that died mid-apply leaves the App itself wrong, not just a
    /// suspect snapshot — the operator needs to be told which way out exists.
    #[test]
    fn a_fired_restore_marker_says_the_app_may_be_half_overwritten() {
        let warning = warning_for("restore Mon Sep  1 12:00:00 UTC 2026\n");
        assert!(
            warning.contains("a previous restore's host-side deadman fired"),
            "{warning}"
        );
        assert!(
            warning.contains("Mon Sep  1 12:00:00 UTC 2026"),
            "{warning}"
        );
        assert!(warning.contains("half-overwritten"), "{warning}");
        assert!(warning.contains("emergency backup"), "{warning}");
        assert!(
            !warning.contains("possibly incomplete"),
            "backup's wording must not leak onto a restore fire: {warning}"
        );
    }

    /// A marker written by a pre-#775 binary carries a bare timestamp. It is
    /// still a fire, and the one thing that must never happen to a fire is
    /// going unreported.
    #[test]
    fn a_marker_holding_only_a_timestamp_still_warns() {
        let warning = warning_for("Mon Sep  1 12:00:00 UTC 2026\n");
        assert!(
            warning.contains("Mon Sep  1 12:00:00 UTC 2026"),
            "{warning}"
        );
        assert!(warning.contains("does not name the operation"), "{warning}");
    }
}
