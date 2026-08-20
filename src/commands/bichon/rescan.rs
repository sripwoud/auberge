use crate::config::Config;
use crate::hosts::{HOST_FLAG, Host, HostManager, select_or_arg};
use crate::output::{self, OutputFormat};
use crate::prompt;
use crate::services::bichon::api::BichonApiClient;
use crate::services::bichon::derive_base_url;
use crate::services::bichon::rescan::{
    ARCHIVE_SERVICE, AccountReport, RescanOutcome, RescanRun, execute_rescan,
};
use crate::services::ssh::LiveSshSession;
use eyre::{Result, WrapErr};
use serde::Serialize;

const ALL_ACCOUNTS: &str = "All accounts";

pub async fn run_rescan(
    host_arg: Option<String>,
    account: Option<String>,
    output: OutputFormat,
) -> Result<i32> {
    match rescan_inner(host_arg, account, output).await {
        Ok(code) => Ok(code),
        Err(err) => {
            output::warn(&format!("rescan failed: {err:#}"));
            Ok(2)
        }
    }
}

async fn rescan_inner(
    host_arg: Option<String>,
    account_filter: Option<String>,
    output: OutputFormat,
) -> Result<i32> {
    let host = resolve_host(host_arg)?;

    let config = Config::load()?;
    let token = config
        .get_resolved("bichon_api_token")?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| eyre::eyre!("bichon_api_token not set in config.toml"))?;
    let base_url = derive_base_url(&config, &host)?;
    let client = BichonApiClient::new(base_url, token)?;

    let mut accounts = client.list_accounts().await?;
    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    let known: Vec<String> = accounts.into_iter().map(|a| a.email).collect();
    if known.is_empty() {
        eyre::bail!("Bichon reports no accounts on '{}'", host.name);
    }

    let selected = resolve_accounts(account_filter.clone(), &known, HostManager::is_tty())?;

    let ssh_key = crate::services::ssh::resolve_ssh_key_path(&host, None)?;
    let ssh = LiveSshSession::new(&host, &ssh_key);

    output::info(&format!(
        "resetting {} archive cursor(s) on {} and starting {} — this waits for the full pass",
        selected.len(),
        host.name,
        ARCHIVE_SERVICE
    ));

    match execute_rescan(&ssh, &selected, &known)? {
        RescanOutcome::Refused { stale_sidecars } => {
            output::warn(&format!(
                "refusing to rescan: {} sidecar(s) lack message_id (e.g. {}). Deploy the current bichon role and let the hourly archive run backfill them (or start {} once), then retry.",
                stale_sidecars.len(),
                stale_sidecars[0],
                ARCHIVE_SERVICE
            ));
            Ok(2)
        }
        RescanOutcome::Busy => {
            output::warn(&format!(
                "an archive run is already in progress; retry once {ARCHIVE_SERVICE} is inactive"
            ));
            Ok(2)
        }
        RescanOutcome::Ran(run) => {
            emit_output(&host.name, account_filter.as_deref(), &run, output)?;
            Ok(run.status().exit_code())
        }
    }
}

fn resolve_host(arg: Option<String>) -> Result<Host> {
    if arg.is_none() && !HostManager::is_tty() {
        eyre::bail!("--host is required when stdin is not a TTY");
    }
    select_or_arg(arg, HOST_FLAG)
}

fn resolve_accounts(filter: Option<String>, known: &[String], is_tty: bool) -> Result<Vec<String>> {
    match filter {
        Some(email) => {
            if known.iter().any(|k| k == &email) {
                Ok(vec![email])
            } else {
                eyre::bail!(
                    "unknown account '{}'; Bichon reports: {}",
                    email,
                    known.join(", ")
                )
            }
        }
        None if !is_tty => Ok(known.to_vec()),
        None => {
            let mut items = Vec::with_capacity(known.len() + 1);
            items.push(ALL_ACCOUNTS.to_string());
            items.extend_from_slice(known);
            let choice = prompt::select_item(
                &items,
                |s: &String| s.clone(),
                prompt::Choice::new("account").resolved_by("--account <email>"),
            )?;
            if choice == ALL_ACCOUNTS {
                Ok(known.to_vec())
            } else {
                Ok(vec![choice])
            }
        }
    }
}

#[derive(Serialize)]
struct RescanOutputDoc<'a> {
    host: &'a str,
    account: Option<&'a str>,
    status: &'static str,
    accounts: &'a [AccountReport],
    total_failures: Option<u64>,
}

fn emit_output(
    host: &str,
    account: Option<&str>,
    run: &RescanRun,
    output: OutputFormat,
) -> Result<()> {
    let status = run.status();
    match output {
        OutputFormat::Json => {
            let doc = RescanOutputDoc {
                host,
                account,
                status: status.as_str(),
                accounts: &run.accounts,
                total_failures: run.total_failures,
            };
            let json = serde_json::to_string_pretty(&doc)
                .wrap_err("failed to serialize rescan output as JSON")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            for a in &run.accounts {
                println!(
                    "{}: {} processed={} skipped={} failures={}",
                    a.email,
                    if a.cursor_reset {
                        "cursor reset,"
                    } else {
                        "cursor kept,"
                    },
                    a.processed,
                    a.skipped,
                    a.failures
                );
            }
            let processed: u64 = run.accounts.iter().map(|a| a.processed).sum();
            let failures = run
                .total_failures
                .map_or_else(|| "unknown".to_string(), |n| n.to_string());
            println!(
                "\nRescan {}: {} processed, {} failure(s) across {} account(s).",
                status.as_str(),
                processed,
                failures,
                run.accounts.len()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Vec<String> {
        vec!["a@x.io".to_string(), "b@x.io".to_string()]
    }

    #[test]
    fn missing_host_off_a_tty_names_the_flag() {
        // cargo test runs with non-TTY stdin, so the guard takes effect.
        let err = resolve_host(None).unwrap_err();
        assert!(format!("{err}").contains("--host"));
    }

    #[test]
    fn explicit_account_must_be_one_bichon_reports() {
        let err = resolve_accounts(Some("ghost@x.io".to_string()), &known(), false).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("ghost@x.io"));
        assert!(msg.contains("a@x.io"));
    }

    #[test]
    fn explicit_known_account_narrows_to_it() {
        let selected = resolve_accounts(Some("b@x.io".to_string()), &known(), false).unwrap();
        assert_eq!(selected, vec!["b@x.io".to_string()]);
    }

    #[test]
    fn omitted_account_off_a_tty_means_all_accounts() {
        let selected = resolve_accounts(None, &known(), false).unwrap();
        assert_eq!(selected, known());
    }

    #[test]
    fn json_doc_carries_the_load_bearing_fields() {
        let run = RescanRun {
            accounts: vec![AccountReport {
                email: "a@x.io".to_string(),
                cursor_reset: true,
                processed: 3,
                skipped: 1,
                failures: 0,
            }],
            total_failures: Some(0),
            service_success: true,
        };
        let doc = RescanOutputDoc {
            host: "auberge",
            account: None,
            status: run.status().as_str(),
            accounts: &run.accounts,
            total_failures: run.total_failures,
        };
        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["status"], "clean");
        assert_eq!(value["accounts"][0]["processed"], 3);
        assert_eq!(value["accounts"][0]["cursor_reset"], true);
        assert_eq!(value["total_failures"], 0);
    }
}
