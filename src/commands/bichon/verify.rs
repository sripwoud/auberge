use crate::config::Config;
use crate::hosts::HostManager;
use crate::output::{self, OutputFormat};
use crate::services::bichon::api::BichonApiClient;
use crate::services::bichon::coverage::{
    CoverageReport, compare_coverage, parse_sidecar_rows, sidecar_rows_command,
    validate_archive_path_for_shell,
};
use crate::services::bichon::derive_base_url;
use crate::services::bichon::rescan::{sanitize_email, validate_email_for_shell};
use crate::services::ssh::{LiveSshSession, SshSession};
use chrono::{Datelike, NaiveDate};
use eyre::{Result, WrapErr};
use serde::Serialize;

pub async fn run_verify_coverage(
    host: String,
    account: String,
    folder: String,
    before: String,
    archive_path: String,
    output: OutputFormat,
) -> Result<i32> {
    match verify_inner(host, account, folder, before, archive_path, output).await {
        Ok(code) => Ok(code),
        Err(err) => {
            output::warn(&format!("verify-coverage failed: {err:#}"));
            Ok(2)
        }
    }
}

async fn verify_inner(
    host: String,
    account: String,
    folder: String,
    before: String,
    archive_path: String,
    output: OutputFormat,
) -> Result<i32> {
    let cutoff = NaiveDate::parse_from_str(&before, "%Y-%m-%d")
        .wrap_err_with(|| format!("--before must be a YYYY-MM-DD date, got '{before}'"))?;
    validate_email_for_shell(&account)?;
    validate_archive_path_for_shell(&archive_path)?;

    let host_record =
        HostManager::get_host(&host).wrap_err_with(|| format!("unknown host '{host}'"))?;
    let config = Config::load()?;
    let token = config
        .get_resolved("bichon_api_token")?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| eyre::eyre!("bichon_api_token not set in config.toml"))?;
    let base_url = derive_base_url(&config, &host_record)?;
    let client = BichonApiClient::new(base_url, token)?;

    let ssh_key = crate::commands::backup::resolve_ssh_key_path(&host_record, None)?;
    let ssh = LiveSshSession::new(&host_record, &ssh_key);

    let report = compute_coverage(&client, &ssh, &account, &folder, cutoff, &archive_path).await?;
    emit_output(&host, &account, &folder, &before, &report, output)?;
    Ok(report.status().exit_code())
}

/// The whole verdict behind the seams the tests can reach: the Bichon API
/// (wiremock) and the Host walk (`SshSession`).
async fn compute_coverage(
    client: &BichonApiClient,
    ssh: &dyn SshSession,
    account: &str,
    folder: &str,
    cutoff: NaiveDate,
    archive_path: &str,
) -> Result<CoverageReport> {
    let accounts = client.list_accounts().await?;
    let account_id = accounts
        .iter()
        .find(|a| a.email == account)
        .map(|a| a.id)
        .ok_or_else(|| {
            eyre::eyre!(
                "Bichon knows no account '{}'; it reports: {}",
                account,
                accounts
                    .iter()
                    .map(|a| a.email.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    // The window is "strictly older than the cutoff date"; Bichon's `before`
    // bound is inclusive, so back off the midnight timestamp by one.
    let cutoff_ms = cutoff
        .and_hms_opt(0, 0, 0)
        .expect("midnight is a valid time")
        .and_utc()
        .timestamp_millis()
        - 1;
    let envelopes = client.search_messages(account_id, cutoff_ms).await?;

    let archive_dir = format!(
        "{}/{}",
        archive_path.trim_end_matches('/'),
        sanitize_email(account)
    );
    let walk = ssh.run(&sidecar_rows_command(&archive_dir))?;
    if !walk.success {
        if walk.exit_code == Some(3) {
            eyre::bail!(
                "no Email Archive directory at {archive_dir}; the archive can vouch for nothing"
            );
        }
        eyre::bail!(
            "could not read the archived sidecars under {archive_dir}: {}",
            walk.stderr_str().trim()
        );
    }
    let rows = parse_sidecar_rows(&walk.stdout_str())?;

    compare_coverage(&envelopes, &rows, folder, (cutoff.year(), cutoff.month()))
}

#[derive(Serialize)]
struct VerifyOutputDoc<'a> {
    host: &'a str,
    account: &'a str,
    folder: &'a str,
    before: &'a str,
    status: &'static str,
    #[serde(flatten)]
    report: &'a CoverageReport,
}

fn emit_output(
    host: &str,
    account: &str,
    folder: &str,
    before: &str,
    report: &CoverageReport,
    output: OutputFormat,
) -> Result<()> {
    let status = report.status();
    match output {
        OutputFormat::Json => {
            let doc = VerifyOutputDoc {
                host,
                account,
                folder,
                before,
                status: status.as_str(),
                report,
            };
            let json = serde_json::to_string_pretty(&doc)
                .wrap_err("failed to serialize verify-coverage output as JSON")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            println!(
                "{account} {folder} before {before}: {} store message(s), {} matched, {} missing",
                report.store_messages,
                report.matched,
                report.missing.len()
            );
            for m in &report.missing {
                println!("missing: {} (date {}, uid {})", m.message_id, m.date, m.uid);
            }
            if report.unverifiable.store_synthetic > 0 || report.unverifiable.archive_sha256 > 0 {
                println!(
                    "unverifiable by identity: {} synthetic store message(s) vs {} sha256-keyed sidecar(s)",
                    report.unverifiable.store_synthetic, report.unverifiable.archive_sha256
                );
            }
            println!("status: {}", status.as_str());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compute_coverage;
    use crate::services::bichon::api::BichonApiClient;
    use crate::services::bichon::coverage::CoverageStatus;
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};
    use chrono::NaiveDate;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cutoff() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 13).unwrap()
    }

    fn stdout(text: &str) -> CommandResult {
        CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    async fn mount_accounts(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{"id": 7, "email": "me@x.io", "sync_folders": ["INBOX"]}],
                "total_items": 1
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_covered_folder_reports_covered() {
        let server = MockServer::start().await;
        mount_accounts(&server).await;

        // 2026-05-13T00:00:00Z is 1778630400000; the inclusive bound backs
        // off by one so a message dated exactly at midnight stays outside.
        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({
                "filter": {"account_ids": [7], "before": 1778630399999i64}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{
                    "id": "e1", "message_id": "a@x.io", "mailbox_name": "INBOX",
                    "uid": 1, "date": 1767225600000i64, "internal_date": 0
                }],
                "total_pages": 1,
                "total_items": 1
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(stdout(
            "/var/lib/bichon-archive/me@x.io/2026/01/1.meta.json\tINBOX\ta@x.io\n",
        ));

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let report = compute_coverage(
            &client,
            &ssh,
            "me@x.io",
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Covered);
        assert_eq!(report.matched, 1);

        let calls = ssh.calls();
        assert_eq!(calls.len(), 1);
        let SshOp::Run(cmd) = &calls[0] else {
            panic!("expected a run call");
        };
        assert!(cmd.contains("/var/lib/bichon-archive/me@x.io"));
    }

    #[tokio::test]
    async fn a_store_message_the_archive_lacks_is_a_gap() {
        let server = MockServer::start().await;
        mount_accounts(&server).await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{
                    "id": "e1", "message_id": "ghost@x.io", "mailbox_name": "INBOX",
                    "uid": 9, "date": 1767225600000i64, "internal_date": 0
                }],
                "total_pages": 1,
                "total_items": 1
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(stdout(""));

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let report = compute_coverage(
            &client,
            &ssh,
            "me@x.io",
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Gap);
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0].message_id, "ghost@x.io");
    }

    #[tokio::test]
    async fn an_unknown_account_names_what_bichon_reports() {
        let server = MockServer::start().await;
        mount_accounts(&server).await;

        let ssh = MockSshSession::new();
        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let err = compute_coverage(
            &client,
            &ssh,
            "ghost@x.io",
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap_err();

        let msg = format!("{err}");
        assert!(msg.contains("ghost@x.io"));
        assert!(msg.contains("me@x.io"));
        assert!(ssh.calls().is_empty());
    }

    #[tokio::test]
    async fn a_missing_archive_directory_is_an_operational_error() {
        let server = MockServer::start().await;
        mount_accounts(&server).await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [],
                "total_pages": 0,
                "total_items": 0
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(3),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let err = compute_coverage(
            &client,
            &ssh,
            "me@x.io",
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap_err();

        assert!(format!("{err}").contains("no Email Archive directory"));
    }
}
