use eyre::{Context, Result};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize)]
pub struct ResticStatus {
    pub percent_done: f64,
    pub total_bytes: Option<u64>,
    pub bytes_done: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ResticSummary {
    pub snapshot_id: String,
    #[allow(dead_code)]
    pub files_new: u64,
    #[allow(dead_code)]
    pub files_changed: u64,
    #[allow(dead_code)]
    pub data_added: u64,
}

#[derive(Debug, Deserialize)]
pub struct ResticExitError {
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "message_type", rename_all = "snake_case")]
pub enum ResticMessage {
    Status(ResticStatus),
    Summary(ResticSummary),
    ExitError(ResticExitError),
}

pub fn parse_restic_message(line: &str) -> Option<ResticMessage> {
    serde_json::from_str(line).ok()
}

/// A `restic` invocation carrying the repository credentials.
///
/// `RESTIC_PASSWORD_COMMAND` is removed because restic prefers it over
/// `RESTIC_PASSWORD`: an operator with it exported would have every auberge
/// restic call resolve the wrong password.
pub fn command(repo: &str, password: &str) -> Command {
    let mut cmd = Command::new("restic");
    cmd.env("RESTIC_REPOSITORY", repo)
        .env("RESTIC_PASSWORD", password)
        .env_remove("RESTIC_PASSWORD_COMMAND");
    cmd
}

/// Human-readable reason from a failed `restic --json` invocation.
///
/// With `--json` restic reports failures as an `exit_error` message on stderr;
/// older versions print plain text, which is passed through unchanged.
pub fn error_message(stderr: &str) -> String {
    stderr
        .lines()
        .find_map(|line| match parse_restic_message(line) {
            Some(ResticMessage::ExitError(err)) => Some(err.message),
            _ => None,
        })
        .unwrap_or_else(|| stderr.trim().to_string())
}

/// Raw `restic snapshots --json` output for the repository.
pub fn snapshots_json(repo: &str, password: &str) -> Result<String> {
    let output = command(repo, password)
        .arg("snapshots")
        .arg("--json")
        .output()
        .wrap_err("Failed to run restic. Install restic: https://restic.net")?;

    if !output.status.success() {
        let reason = error_message(&String::from_utf8_lossy(&output.stderr));
        eyre::bail!(match reason.is_empty() {
            true => format!("restic snapshots failed ({})", output.status),
            false => reason,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether `path` is present inside a snapshot.
///
/// `restic ls <id> <dir>` walks only that subtree and exits 0 whether or not
/// the dir exists, so presence is read off the output. Streaming stops at the
/// first hit — an app's subtree can hold hundreds of thousands of files.
/// restic's own diagnostics stay on the inherited stderr.
pub fn snapshot_contains_path(
    repo: &str,
    password: &str,
    snapshot_id: &str,
    path: &str,
) -> Result<bool> {
    let mut child = ls_command(repo, password, snapshot_id, path)
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("Failed to run restic. Install restic: https://restic.net")?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre::eyre!("Failed to capture restic ls output"))?;

    let found = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| is_path_under(&line, path));

    if found {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(true);
    }

    let status = child.wait().wrap_err("Failed to wait on restic ls")?;
    if !status.success() {
        eyre::bail!("restic ls failed ({status})");
    }

    Ok(false)
}

/// `--no-lock` because the caller SIGKILLs on first match: a killed `ls`
/// skips restic's cleanup and would leave its default non-exclusive lock
/// behind on every matching verify.
fn ls_command(repo: &str, password: &str, snapshot_id: &str, path: &str) -> Command {
    let mut cmd = command(repo, password);
    cmd.arg("ls").arg("--no-lock").arg(snapshot_id).arg(path);
    cmd
}

/// Whether a `restic ls` line is `path` itself or a descendant of it. The
/// header line (`snapshot <id> of […]`) and sibling prefixes never match.
fn is_path_under(line: &str, path: &str) -> bool {
    line == path
        || line
            .strip_prefix(path)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_restic_status_line() {
        let line = r#"{"message_type":"status","percent_done":0.5,"total_bytes":1048576,"bytes_done":524288}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => {
                assert!((s.percent_done - 0.5).abs() < f64::EPSILON);
                assert_eq!(s.total_bytes, Some(1048576));
                assert_eq!(s.bytes_done, Some(524288));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_restic_summary_line() {
        let line = r#"{"message_type":"summary","snapshot_id":"abc123","files_new":10,"files_changed":2,"data_added":1048576}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Summary(s) => {
                assert_eq!(s.snapshot_id, "abc123");
                assert_eq!(s.files_new, 10);
                assert_eq!(s.files_changed, 2);
                assert_eq!(s.data_added, 1048576);
            }
            _ => panic!("expected Summary"),
        }
    }

    #[test]
    fn parse_restic_plain_text_returns_none() {
        assert!(parse_restic_message("using parent snapshot abc123").is_none());
    }

    #[test]
    fn parse_restic_malformed_json_returns_none() {
        assert!(parse_restic_message("{bad json}").is_none());
    }

    #[test]
    fn parse_restic_zero_percent() {
        let line = r#"{"message_type":"status","percent_done":0.0}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => {
                assert!((s.percent_done).abs() < f64::EPSILON);
                assert_eq!(s.total_bytes, None);
                assert_eq!(s.bytes_done, None);
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_restic_exit_error_line() {
        let line = r#"{"message_type":"exit_error","code":12,"message":"Fatal: wrong password or no key found"}"#;
        match parse_restic_message(line).unwrap() {
            ResticMessage::ExitError(err) => {
                assert_eq!(err.message, "Fatal: wrong password or no key found");
            }
            _ => panic!("expected ExitError"),
        }
    }

    #[test]
    fn command_sets_repo_and_password_and_drops_password_command() {
        let cmd = command("rclone:filen:auberge-backup", "s3cret");
        let envs: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();

        assert!(envs.contains(&(
            "RESTIC_REPOSITORY".to_string(),
            Some("rclone:filen:auberge-backup".to_string())
        )));
        assert!(envs.contains(&("RESTIC_PASSWORD".to_string(), Some("s3cret".to_string()))));
        assert!(envs.contains(&("RESTIC_PASSWORD_COMMAND".to_string(), None)));
    }

    #[test]
    fn ls_command_passes_no_lock_flag() {
        let cmd = ls_command("rclone:remote:repo", "", "abc123", "/backups/x");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();

        assert_eq!(args, ["ls", "--no-lock", "abc123", "/backups/x"]);
    }

    #[test]
    fn error_message_extracts_exit_error_message() {
        let stderr = r#"{"message_type":"exit_error","code":10,"message":"Fatal: repository does not exist"}"#;
        assert_eq!(error_message(stderr), "Fatal: repository does not exist");
    }

    #[test]
    fn error_message_passes_through_plain_text() {
        assert_eq!(
            error_message("  Fatal: unable to open config file\n"),
            "Fatal: unable to open config file"
        );
    }

    #[test]
    fn error_message_of_empty_stderr_is_empty() {
        assert_eq!(error_message(""), "");
    }

    #[test]
    fn is_path_under_matches_the_path_itself() {
        assert!(is_path_under(
            "/backups/myserver/ts/bichon",
            "/backups/myserver/ts/bichon"
        ));
    }

    #[test]
    fn is_path_under_matches_descendants() {
        assert!(is_path_under(
            "/backups/myserver/ts/bichon/2026/a.eml",
            "/backups/myserver/ts/bichon"
        ));
    }

    #[test]
    fn is_path_under_rejects_ls_header_line() {
        assert!(!is_path_under(
            "snapshot ef9c32e9 of [/backups/myserver/ts] at 2026-07-29 by user:",
            "/backups/myserver/ts/bichon"
        ));
    }

    #[test]
    fn is_path_under_rejects_sibling_sharing_a_prefix() {
        assert!(!is_path_under(
            "/backups/myserver/ts/bichon-archive",
            "/backups/myserver/ts/bichon"
        ));
    }

    #[test]
    fn is_path_under_rejects_ancestor_lines() {
        assert!(!is_path_under(
            "/backups/myserver/ts",
            "/backups/myserver/ts/bichon"
        ));
    }

    #[test]
    fn parse_restic_full_percent() {
        let line =
            r#"{"message_type":"status","percent_done":1.0,"total_bytes":100,"bytes_done":100}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => assert!((s.percent_done - 1.0).abs() < f64::EPSILON),
            _ => panic!("expected Status"),
        }
    }
}
