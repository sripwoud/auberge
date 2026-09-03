use crate::services::ssh::SshSession;
use eyre::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const ARCHIVE_DIR: &str = "/var/lib/bichon-archive";
pub const STATE_DIR: &str = "/var/lib/bichon-archive/.state";
pub const ARCHIVE_SERVICE: &str = "bichon-archive.service";
pub const ARCHIVE_USER: &str = "bichon";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescanStatus {
    Clean,
    RunFailures,
    OperationalError,
}

impl RescanStatus {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Clean => 0,
            Self::RunFailures => 1,
            Self::OperationalError => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::RunFailures => "run_failures",
            Self::OperationalError => "operational_error",
        }
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct AccountReport {
    pub email: String,
    pub cursor_reset: bool,
    pub processed: u64,
    pub skipped: u64,
    pub failures: u64,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct RescanRun {
    pub accounts: Vec<AccountReport>,
    pub total_failures: Option<u64>,
    pub service_success: bool,
}

impl RescanRun {
    pub fn status(&self) -> RescanStatus {
        if self.total_failures.unwrap_or(0) > 0 {
            RescanStatus::RunFailures
        } else if self.service_success {
            RescanStatus::Clean
        } else {
            RescanStatus::OperationalError
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RescanOutcome {
    Refused { stale_sidecars: Vec<String> },
    Busy,
    Ran(RescanRun),
}

pub fn sanitize_email(email: &str) -> String {
    email.replace('/', "_")
}

// Cursor paths and reset commands interpolate the email into a remote shell
// line; rejecting anything beyond the address characters Bichon accepts is
// simpler and stricter than escaping. verify-coverage interpolates the same
// value into its sidecar walk, so it applies the same rule.
pub(crate) fn validate_email_for_shell(email: &str) -> Result<()> {
    let ok = !email.is_empty()
        && email
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | '+' | '/'));
    if !ok {
        eyre::bail!("account email '{email}' contains characters unsafe for a remote shell");
    }
    Ok(())
}

pub fn stale_sidecar_command() -> String {
    format!(
        r#"sudo sh -c '[ -d {ARCHIVE_DIR} ] || exit 0; grep -rL --include="*.meta.json" -e "\"message_id\"" {ARCHIVE_DIR} || true'"#
    )
}

pub fn parse_stale_sidecars(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

pub fn is_service_active(stdout: &str) -> bool {
    matches!(
        stdout.trim(),
        "active" | "activating" | "reloading" | "deactivating"
    )
}

pub fn cursor_reset_command(email: &str) -> String {
    let cursor = format!("{STATE_DIR}/{}.cursor", sanitize_email(email));
    format!(
        r#"sudo -u {ARCHIVE_USER} sh -c 'umask 077; mkdir -p {STATE_DIR} && printf "0\n" > {cursor}'"#
    )
}

pub fn parse_journal_cursor(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("-- cursor: "))
        .map(|cursor| cursor.trim().to_string())
        .filter(|cursor| !cursor.is_empty())
}

fn validate_cursor_for_shell(cursor: &str) -> Result<()> {
    let ok = cursor.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '=' | ';' | '-' | '_' | '.' | ':' | '+' | '/')
    });
    if !ok {
        eyre::bail!("journal cursor '{cursor}' contains characters unsafe for a remote shell");
    }
    Ok(())
}

fn token_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|tok| {
        tok.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

fn token_u64(line: &str, key: &str) -> Option<u64> {
    token_value(line, key)?.parse().ok()
}

pub fn parse_run_report(
    journal: &str,
    known_emails: &[String],
    reset_emails: &HashSet<String>,
) -> (Vec<AccountReport>, Option<u64>) {
    let safe_to_email: HashMap<String, &String> = known_emails
        .iter()
        .map(|email| (sanitize_email(email), email))
        .collect();
    let reset_safe: HashSet<String> = reset_emails.iter().map(|e| sanitize_email(e)).collect();

    let mut accounts = Vec::new();
    let mut total_failures = None;

    for line in journal.lines() {
        if let Some(total) = token_u64(line, "total_failures") {
            total_failures = Some(total);
            continue;
        }
        // The per-account summary is the only line carrying all three
        // counters; backfill and body_repair lines carry failures= too.
        let (Some(safe), Some(processed), Some(skipped), Some(failures)) = (
            token_value(line, "account"),
            token_u64(line, "processed"),
            token_u64(line, "skipped"),
            token_u64(line, "failures"),
        ) else {
            continue;
        };
        let email = safe_to_email
            .get(safe)
            .map_or_else(|| safe.to_string(), |e| (*e).clone());
        accounts.push(AccountReport {
            cursor_reset: reset_safe.contains(safe),
            email,
            processed,
            skipped,
            failures,
        });
    }

    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    (accounts, total_failures)
}

pub fn execute_rescan(
    ssh: &dyn SshSession,
    emails_to_reset: &[String],
    known_emails: &[String],
) -> Result<RescanOutcome> {
    for email in emails_to_reset {
        validate_email_for_shell(email)?;
    }

    let stale_check = ssh.run(&stale_sidecar_command())?;
    if !stale_check.success {
        eyre::bail!(
            "sidecar precondition check failed on the host: {}",
            stale_check.stderr_str().trim()
        );
    }
    let stale = parse_stale_sidecars(&stale_check.stdout_str());
    if !stale.is_empty() {
        return Ok(RescanOutcome::Refused {
            stale_sidecars: stale,
        });
    }

    // `is-active` exits non-zero for inactive units, so only stdout answers
    // the question; transport failures surface on the next command instead.
    let active = ssh.run(&format!("sudo systemctl is-active {ARCHIVE_SERVICE}"))?;
    if is_service_active(&active.stdout_str()) {
        return Ok(RescanOutcome::Busy);
    }

    for email in emails_to_reset {
        let reset = ssh.run(&cursor_reset_command(email))?;
        if !reset.success {
            eyre::bail!(
                "cursor reset failed for {email}: {}",
                reset.stderr_str().trim()
            );
        }
    }

    // The run's report is read from the journal after a cursor captured
    // here, not by InvocationID: systemd drops that property once it
    // unloads an inactive oneshot, so it can be gone by the time the pass
    // finishes. Same pattern as the uidvalidity watch.
    let cursor_capture = ssh.run("sudo journalctl -n0 --show-cursor --quiet --no-pager")?;
    let Some(cursor) = parse_journal_cursor(&cursor_capture.stdout_str()) else {
        eyre::bail!(
            "could not capture a journal cursor: {}",
            cursor_capture.stderr_str().trim()
        );
    };
    validate_cursor_for_shell(&cursor)?;

    // Type=oneshot: start blocks until the archive pass finishes, and a
    // non-zero exit is a verdict to report, not an error to bail on.
    let start = ssh.run(&format!("sudo systemctl start {ARCHIVE_SERVICE}"))?;

    let journal = ssh.run(&format!(
        "sudo journalctl -u {ARCHIVE_SERVICE} --after-cursor '{cursor}' -o cat --no-pager"
    ))?;
    if !journal.success {
        eyre::bail!(
            "could not read the archive run journal: {}",
            journal.stderr_str().trim()
        );
    }

    let reset_set: HashSet<String> = emails_to_reset.iter().cloned().collect();
    let (accounts, total_failures) =
        parse_run_report(&journal.stdout_str(), known_emails, &reset_set);

    Ok(RescanOutcome::Ran(RescanRun {
        accounts,
        total_failures,
        service_success: start.success,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};

    fn ok_with_stdout(stdout: &str) -> CommandResult {
        CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failed_with(stdout: &str, stderr: &str, code: i32) -> CommandResult {
        CommandResult {
            success: false,
            exit_code: Some(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    const CURSOR_STDOUT: &str = "-- cursor: s=0123456789ab;i=1f2;b=00aa;m=bb;t=cc;x=dd\n";

    fn emails(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn refuses_when_any_sidecar_lacks_message_id() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(
            "/var/lib/bichon-archive/a/2024/01/1.meta.json\n/var/lib/bichon-archive/a/2024/02/2.meta.json\n",
        ));

        let outcome = execute_rescan(&mock, &emails(&["a@x.io"]), &emails(&["a@x.io"])).unwrap();

        assert_eq!(
            outcome,
            RescanOutcome::Refused {
                stale_sidecars: vec![
                    "/var/lib/bichon-archive/a/2024/01/1.meta.json".to_string(),
                    "/var/lib/bichon-archive/a/2024/02/2.meta.json".to_string(),
                ]
            }
        );
        // Refusal happens before any cursor is touched or the service started.
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn busy_when_an_archive_run_is_active() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(ok_with_stdout("active\n"));

        let outcome = execute_rescan(&mock, &emails(&["a@x.io"]), &emails(&["a@x.io"])).unwrap();

        assert_eq!(outcome, RescanOutcome::Busy);
        assert_eq!(
            mock.calls()[1],
            SshOp::Run("sudo systemctl is-active bichon-archive.service".to_string())
        );
        assert_eq!(mock.calls().len(), 2);
    }

    #[test]
    fn clean_run_resets_selected_cursor_and_reports_all_accounts() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout(CURSOR_STDOUT));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout(
            "2026-07-31T09:00:00Z account=dev@x.io id=1 cursor_ms=0 since_ms=0\n\
             2026-07-31T09:00:01Z account=dev@x.io backfill repaired=0 failures=0\n\
             2026-07-31T09:05:00Z account=dev@x.io processed=432 skipped=4527 failures=0\n\
             2026-07-31T09:06:00Z account=ops@x.io processed=0 skipped=12 failures=0\n\
             2026-07-31T09:06:01Z total_failures=0\n",
        ));

        let outcome = execute_rescan(
            &mock,
            &emails(&["dev@x.io"]),
            &emails(&["dev@x.io", "ops@x.io"]),
        )
        .unwrap();

        let RescanOutcome::Ran(run) = outcome else {
            panic!("expected Ran, got {outcome:?}");
        };
        assert_eq!(run.status(), RescanStatus::Clean);
        assert_eq!(run.status().exit_code(), 0);
        assert_eq!(run.total_failures, Some(0));
        assert_eq!(
            run.accounts,
            vec![
                AccountReport {
                    email: "dev@x.io".to_string(),
                    cursor_reset: true,
                    processed: 432,
                    skipped: 4527,
                    failures: 0,
                },
                AccountReport {
                    email: "ops@x.io".to_string(),
                    cursor_reset: false,
                    processed: 0,
                    skipped: 12,
                    failures: 0,
                },
            ]
        );

        let calls = mock.calls();
        assert_eq!(
            calls[2],
            SshOp::Run(
                r#"sudo -u bichon sh -c 'umask 077; mkdir -p /var/lib/bichon-archive/.state && printf "0\n" > /var/lib/bichon-archive/.state/dev@x.io.cursor'"#
                    .to_string()
            ),
            "only the selected account's cursor is reset"
        );
        assert_eq!(
            calls[3],
            SshOp::Run("sudo journalctl -n0 --show-cursor --quiet --no-pager".to_string())
        );
        assert_eq!(
            calls[4],
            SshOp::Run("sudo systemctl start bichon-archive.service".to_string())
        );
        assert_eq!(
            calls[5],
            SshOp::Run(
                "sudo journalctl -u bichon-archive.service --after-cursor 's=0123456789ab;i=1f2;b=00aa;m=bb;t=cc;x=dd' -o cat --no-pager"
                    .to_string()
            )
        );
    }

    #[test]
    fn counted_failures_are_exit_one() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout(CURSOR_STDOUT));
        mock.stage_run_result(failed_with("", "Job for bichon-archive.service failed", 1));
        mock.stage_run_result(ok_with_stdout(
            "account=dev@x.io processed=10 skipped=2 failures=3\ntotal_failures=3\n",
        ));

        let outcome =
            execute_rescan(&mock, &emails(&["dev@x.io"]), &emails(&["dev@x.io"])).unwrap();

        let RescanOutcome::Ran(run) = outcome else {
            panic!("expected Ran, got {outcome:?}");
        };
        assert_eq!(run.status(), RescanStatus::RunFailures);
        assert_eq!(run.status().exit_code(), 1);
    }

    #[test]
    fn service_failure_without_a_report_is_operational() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout(CURSOR_STDOUT));
        mock.stage_run_result(failed_with("", "Job failed", 1));
        mock.stage_run_result(ok_with_stdout(
            "2026-07-31T09:00:00Z auth check failed: GET /api/v1/current-user did not return 2xx\n",
        ));

        let outcome =
            execute_rescan(&mock, &emails(&["dev@x.io"]), &emails(&["dev@x.io"])).unwrap();

        let RescanOutcome::Ran(run) = outcome else {
            panic!("expected Ran, got {outcome:?}");
        };
        assert_eq!(run.total_failures, None);
        assert_eq!(run.status(), RescanStatus::OperationalError);
        assert_eq!(run.status().exit_code(), 2);
    }

    #[test]
    fn rejects_email_with_shell_metacharacters() {
        let mock = MockSshSession::new();
        let err = execute_rescan(
            &mock,
            &emails(&["evil'; rm -rf / #@x.io"]),
            &emails(&["evil'; rm -rf / #@x.io"]),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("unsafe"));
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn parse_run_report_maps_sanitized_emails_and_flags_resets() {
        let journal = "account=weird_address@x.io processed=1 skipped=0 failures=0\n\
                       total_failures=0\n";
        let known = emails(&["weird/address@x.io"]);
        let reset: HashSet<String> = known.iter().cloned().collect();

        let (accounts, total) = parse_run_report(journal, &known, &reset);

        assert_eq!(total, Some(0));
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "weird/address@x.io");
        assert!(accounts[0].cursor_reset);
    }

    #[test]
    fn parse_run_report_ignores_backfill_and_body_repair_lines() {
        let journal = "account=a@x.io backfill repaired=2 failures=1\n\
                       account=a@x.io body_repair repaired=1 failures=1\n\
                       account=a@x.io processed=5 skipped=0 failures=0\n\
                       total_failures=2\n";

        let (accounts, total) = parse_run_report(journal, &emails(&["a@x.io"]), &HashSet::new());

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].processed, 5);
        assert_eq!(total, Some(2));
    }

    #[test]
    fn parse_run_report_keeps_unknown_accounts_under_their_safe_name() {
        let journal = "account=ghost@x.io processed=0 skipped=1 failures=0\n";

        let (accounts, total) = parse_run_report(journal, &[], &HashSet::new());

        assert_eq!(total, None);
        assert_eq!(accounts[0].email, "ghost@x.io");
        assert!(!accounts[0].cursor_reset);
    }

    #[test]
    fn cursor_reset_command_writes_zero_as_the_bichon_user() {
        let cmd = cursor_reset_command("dev/x@y.io");
        assert_eq!(
            cmd,
            r#"sudo -u bichon sh -c 'umask 077; mkdir -p /var/lib/bichon-archive/.state && printf "0\n" > /var/lib/bichon-archive/.state/dev_x@y.io.cursor'"#
        );
    }

    #[test]
    fn missing_journal_cursor_is_an_error() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout(""));

        let err = execute_rescan(&mock, &emails(&["a@x.io"]), &emails(&["a@x.io"])).unwrap_err();
        assert!(format!("{err}").contains("journal cursor"));
        // The service is never started without a cursor to scope its report.
        assert_eq!(mock.calls().len(), 4);
    }

    #[test]
    fn unquotable_journal_cursor_is_an_error() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout("-- cursor: s=abc'; rm -rf / #\n"));

        let err = execute_rescan(&mock, &emails(&["a@x.io"]), &emails(&["a@x.io"])).unwrap_err();
        assert!(format!("{err}").contains("unsafe"));
        assert_eq!(mock.calls().len(), 4);
    }

    #[test]
    fn parse_journal_cursor_reads_the_cursor_line() {
        assert_eq!(
            parse_journal_cursor(CURSOR_STDOUT),
            Some("s=0123456789ab;i=1f2;b=00aa;m=bb;t=cc;x=dd".to_string())
        );
        assert_eq!(parse_journal_cursor(""), None);
        assert_eq!(parse_journal_cursor("-- cursor: \n"), None);
    }

    #[test]
    fn unreachable_host_fails_on_the_first_command() {
        let mock = MockSshSession::new();
        mock.stage_run_result(failed_with("", "ssh: connect to host: timed out", 255));

        let err = execute_rescan(&mock, &emails(&["a@x.io"]), &emails(&["a@x.io"])).unwrap_err();
        assert!(format!("{err}").contains("precondition"));
        assert_eq!(mock.calls().len(), 1);
    }
}
