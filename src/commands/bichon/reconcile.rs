use crate::config::Config;
use crate::hosts::HostManager;
use crate::output::OutputFormat;
use crate::services::bichon::api::{Account, BichonApiClient};
use crate::services::bichon::folder_filter::is_excluded;
use eyre::{Result, WrapErr};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize)]
struct AccountPlan {
    email: String,
    added: Vec<String>,
    removed: Vec<String>,
    unchanged: Vec<String>,
}

#[derive(Serialize)]
struct ReconcileSummary {
    added: usize,
    removed: usize,
    changed_accounts: usize,
}

#[derive(Serialize)]
struct ReconcileOutput {
    host: String,
    apply: bool,
    account: Option<String>,
    accounts: Vec<AccountPlan>,
    summary: ReconcileSummary,
}

pub async fn run_reconcile_folders(
    host: String,
    apply: bool,
    account_filter: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    HostManager::get_host(&host).wrap_err_with(|| format!("unknown host '{host}'"))?;

    let config = Config::load()?;
    let token = config
        .get_resolved("bichon_api_token")?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| eyre::eyre!("bichon_api_token not set in config.toml"))?;
    let base_url = derive_bichon_base_url(&config)?;

    let client = BichonApiClient::new(base_url, token)?;
    let mut accounts = client.list_accounts().await?;
    accounts.sort_by(|a, b| a.email.cmp(&b.email));

    let accounts: Vec<Account> = if let Some(email) = &account_filter {
        accounts.into_iter().filter(|a| &a.email == email).collect()
    } else {
        accounts
    };

    let mut plans = Vec::new();
    let mut total_added = 0usize;
    let mut total_removed = 0usize;
    let mut changed_accounts = 0usize;

    for account in accounts {
        let mailbox_list = client.list_mailboxes(account.id).await?;
        let extra_excluded: HashSet<String> = config
            .bichon_extra_excluded_folders(&account.email)
            .into_iter()
            .collect();

        let desired_set: HashSet<String> = mailbox_list
            .iter()
            .filter(|mb| !is_excluded(mb, &extra_excluded))
            .map(|mb| mb.name.clone())
            .collect();
        let current_set: HashSet<String> = account.sync_folders.iter().cloned().collect();

        let mut added: Vec<String> = desired_set.difference(&current_set).cloned().collect();
        let mut removed: Vec<String> = current_set.difference(&desired_set).cloned().collect();
        let mut unchanged: Vec<String> = current_set.intersection(&desired_set).cloned().collect();
        let mut desired_sorted: Vec<String> = desired_set.into_iter().collect();
        added.sort();
        removed.sort();
        unchanged.sort();
        desired_sorted.sort();

        if !added.is_empty() || !removed.is_empty() {
            changed_accounts += 1;
            total_added += added.len();
            total_removed += removed.len();
            if apply {
                client
                    .update_account_sync_folders(account.id, &desired_sorted)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to update sync_folders for {}", account.email)
                    })?;
            }
        }

        plans.push(AccountPlan {
            email: account.email,
            added,
            removed,
            unchanged,
        });
    }

    let result = ReconcileOutput {
        host,
        apply,
        account: account_filter,
        accounts: plans,
        summary: ReconcileSummary {
            added: total_added,
            removed: total_removed,
            changed_accounts,
        },
    };
    emit_output(result, output);
    Ok(())
}

fn derive_bichon_base_url(config: &Config) -> Result<String> {
    if let Some(base_url) = config
        .get("bichon_base_url")
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(base_url.trim_end_matches('/').to_string());
    }
    let domain = config.domain();
    if domain.is_empty() {
        eyre::bail!("domain not set in config.toml")
    }
    let subdomain = config
        .get("bichon_subdomain")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "bichon".to_string());
    Ok(format!("https://{}.{}", subdomain.trim(), domain))
}

fn emit_output(result: ReconcileOutput, output: OutputFormat) {
    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        OutputFormat::Human => {
            for account in &result.accounts {
                println!("{}:", account.email);
                if !account.added.is_empty() {
                    println!("  + Add to sync_folders: {:?}", account.added);
                }
                if !account.removed.is_empty() {
                    println!("  - Remove from sync_folders: {:?}", account.removed);
                }
                println!("  unchanged: {:?}", account.unchanged);
            }
            if result.apply {
                println!(
                    "\nApplied: {} added, {} removed across {} account(s).",
                    result.summary.added, result.summary.removed, result.summary.changed_accounts
                );
            } else {
                println!(
                    "\nPlan: {} added, {} removed across {} account(s). Run with --apply to commit.",
                    result.summary.added, result.summary.removed, result.summary.changed_accounts
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_reconcile_folders;
    use crate::output::OutputFormat;
    use eyre::Result;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;
    use wiremock::matchers::{body_json, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn prepare_config(server: &MockServer) -> Result<TempDir> {
        let tmp = tempfile::tempdir()?;
        let config_home = tmp.path().join("cfg");
        let data_home = tmp.path().join("data");
        fs::create_dir_all(config_home.join("auberge"))?;
        fs::create_dir_all(&data_home)?;
        fs::write(
            config_home.join("auberge/config.toml"),
            format!(
                r#"
domain = "example.com"
bichon_api_token = "token-123"
bichon_base_url = "{}"
[bichon.account_overrides."me@sripwoud.xyz"]
extra_excluded_folders = ["Receipts/2019"]
"#,
                server.uri()
            ),
        )?;
        fs::write(
            config_home.join("auberge/hosts.toml"),
            r#"
[[hosts]]
name = "auberge"
address = "100.100.100.10"
user = "root"
tailscale_ip = "100.100.100.10"
"#,
        )?;

        // SAFETY: tests in this module mutate env vars serially by default.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            std::env::set_var("XDG_DATA_HOME", &data_home);
        }
        Ok(tmp)
    }

    #[tokio::test]
    async fn dry_run_outputs_expected_json_diff() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _tmp = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX","Sent","INBOX/old-archive","Receipts/2019"]},
                {"id":2, "email":"work@sripwoud.xyz", "sync_folders":["INBOX","Sent"]}
            ])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/list-mailboxes/1"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"INBOX","attributes":[]},
                {"name":"Sent","attributes":[]},
                {"name":"INBOX/legal-2026","attributes":[]},
                {"name":"INBOX/old-archive","attributes":[{"kind":"Trash"}]},
                {"name":"Receipts/2019","attributes":[]}
            ])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/list-mailboxes/2"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"INBOX","attributes":[]},
                {"name":"Sent","attributes":[]}
            ])))
            .mount(&server)
            .await;

        run_reconcile_folders(
            "auberge".to_string(),
            false,
            Some("me@sripwoud.xyz".to_string()),
            OutputFormat::Json,
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn apply_patches_sync_folders_once_when_changed() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _tmp = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX","INBOX/old-archive"]}
            ])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/list-mailboxes/1"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"INBOX","attributes":[]},
                {"name":"INBOX/legal-2026","attributes":[]},
                {"name":"INBOX/old-archive","attributes":[{"kind":"Trash"}]}
            ])))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v1/account/1"))
            .and(body_json(
                json!({"sync_folders":["INBOX","INBOX/legal-2026"]}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;

        run_reconcile_folders("auberge".to_string(), true, None, OutputFormat::Json).await?;
        Ok(())
    }

    #[tokio::test]
    async fn apply_is_idempotent_with_no_diff() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _tmp = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX","INBOX/legal-2026"]}
            ])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/list-mailboxes/1$"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"INBOX","attributes":[]},
                {"name":"INBOX/legal-2026","attributes":[]}
            ])))
            .mount(&server)
            .await;

        run_reconcile_folders("auberge".to_string(), true, None, OutputFormat::Json).await?;
        Ok(())
    }
}
