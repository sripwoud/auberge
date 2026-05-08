use crate::config::Config;
use crate::hosts::{Host, HostManager};
use crate::output::OutputFormat;
use crate::services::bichon::api::{Account, BichonApiClient};
use crate::services::bichon::folder_filter::is_excluded;
use eyre::{Result, WrapErr};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct AccountPlan {
    pub email: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub added: usize,
    pub removed: usize,
    pub changed_accounts: usize,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct ReconcileOutput {
    pub host: String,
    pub apply: bool,
    pub account: Option<String>,
    pub accounts: Vec<AccountPlan>,
    pub summary: ReconcileSummary,
}

pub async fn run_reconcile_folders(
    host: String,
    apply: bool,
    account_filter: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    let result = compute_reconcile(host, apply, account_filter).await?;
    emit_output(&result, output)?;
    Ok(())
}

pub async fn compute_reconcile(
    host: String,
    apply: bool,
    account_filter: Option<String>,
) -> Result<ReconcileOutput> {
    let host_record =
        HostManager::get_host(&host).wrap_err_with(|| format!("unknown host '{host}'"))?;

    let config = Config::load()?;
    let token = config
        .get_resolved("bichon_api_token")?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| eyre::eyre!("bichon_api_token not set in config.toml"))?;
    let base_url = derive_bichon_base_url(&config, &host_record)?;

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

    Ok(ReconcileOutput {
        host,
        apply,
        account: account_filter,
        accounts: plans,
        summary: ReconcileSummary {
            added: total_added,
            removed: total_removed,
            changed_accounts,
        },
    })
}

fn derive_bichon_base_url(config: &Config, host: &Host) -> Result<String> {
    if let Some(per_host) = config.bichon_host_base_url(&host.name) {
        return Ok(per_host);
    }
    if let Some(base_url) = config
        .get("bichon_base_url")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Ok(base_url.trim_end_matches('/').to_string());
    }
    let domain = config.domain();
    if domain.is_empty() {
        eyre::bail!(
            "no Bichon base URL configured for host '{}'. Set [bichon.hosts.\"{}\"] base_url, or set bichon_base_url, or set domain in config.toml",
            host.name,
            host.name
        )
    }
    let subdomain = config
        .get("bichon_subdomain")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "bichon".to_string());
    Ok(format!("https://{subdomain}.{domain}"))
}

fn emit_output(result: &ReconcileOutput, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(result)
                .wrap_err("failed to serialize reconcile output as JSON")?;
            println!("{json}");
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AccountPlan, ReconcileSummary, compute_reconcile};
    use crate::output::EnvVarGuard;
    use eyre::Result;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;
    use wiremock::matchers::{body_json, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct TestEnv {
        _tmp: TempDir,
        _xdg_config: EnvVarGuard,
        _xdg_data: EnvVarGuard,
    }

    fn prepare_config(server: &MockServer, config_extra: &str) -> Result<TestEnv> {
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
[bichon.hosts.auberge]
base_url = "{}"
[bichon.account_overrides."me@sripwoud.xyz"]
extra_excluded_folders = ["Receipts/2019"]
{}
"#,
                server.uri(),
                config_extra
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

        // Caller MUST hold TEST_LOCK; guards restore previous env values on Drop.
        let xdg_config = EnvVarGuard::set("XDG_CONFIG_HOME", &config_home);
        let xdg_data = EnvVarGuard::set("XDG_DATA_HOME", &data_home);
        Ok(TestEnv {
            _tmp: tmp,
            _xdg_config: xdg_config,
            _xdg_data: xdg_data,
        })
    }

    #[tokio::test]
    async fn dry_run_returns_expected_diff() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server, "")?;

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

        Mock::given(method("POST"))
            .and(path_regex(r"^/api/v1/account/\d+$"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let result = compute_reconcile(
            "auberge".to_string(),
            false,
            Some("me@sripwoud.xyz".to_string()),
        )
        .await?;

        assert!(!result.apply);
        assert_eq!(result.host, "auberge");
        assert_eq!(result.account.as_deref(), Some("me@sripwoud.xyz"));
        assert_eq!(result.accounts.len(), 1);
        let plan = &result.accounts[0];
        assert_eq!(plan.email, "me@sripwoud.xyz");
        assert_eq!(plan.added, vec!["INBOX/legal-2026".to_string()]);
        // excluded by SPECIAL-USE \Trash and by extra_excluded_folders override
        let mut removed = plan.removed.clone();
        removed.sort();
        assert_eq!(
            removed,
            vec!["INBOX/old-archive".to_string(), "Receipts/2019".to_string()]
        );
        assert_eq!(
            plan.unchanged,
            vec!["INBOX".to_string(), "Sent".to_string()]
        );
        assert_eq!(
            result.summary,
            ReconcileSummary {
                added: 1,
                removed: 2,
                changed_accounts: 1
            }
        );
        Ok(())
    }

    #[tokio::test]
    async fn apply_patches_sync_folders_once_when_changed() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server, "")?;

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

        let result = compute_reconcile("auberge".to_string(), true, None).await?;
        assert!(result.apply);
        assert_eq!(result.summary.changed_accounts, 1);
        assert_eq!(result.summary.added, 1);
        assert_eq!(result.summary.removed, 1);
        Ok(())
    }

    #[tokio::test]
    async fn apply_is_idempotent_with_no_diff() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server, "")?;

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

        // Critical: assert NO POST is sent when there is no diff.
        Mock::given(method("POST"))
            .and(path_regex(r"^/api/v1/account/\d+$"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let result = compute_reconcile("auberge".to_string(), true, None).await?;
        assert!(result.apply);
        assert_eq!(
            result.summary,
            ReconcileSummary {
                added: 0,
                removed: 0,
                changed_accounts: 0
            }
        );
        let plan = &result.accounts[0];
        assert!(plan.added.is_empty());
        assert!(plan.removed.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn retries_on_5xx_then_succeeds() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server, "")?;

        // First call: 500. Second call: success.
        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        let result = compute_reconcile("auberge".to_string(), false, None).await?;
        assert!(result.accounts.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn unknown_host_fails_before_network() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server, "")?;

        let err = compute_reconcile("not-a-host".to_string(), false, None)
            .await
            .unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("not-a-host"),
            "expected error to mention host name, got: {msg}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn falls_back_to_global_bichon_base_url_when_per_host_missing() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
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
"#,
        )?;
        let _xdg_config = EnvVarGuard::set("XDG_CONFIG_HOME", &config_home);
        let _xdg_data = EnvVarGuard::set("XDG_DATA_HOME", &data_home);

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;

        let result = compute_reconcile("auberge".to_string(), false, None).await?;
        assert_eq!(result.accounts, Vec::<AccountPlan>::new());
        Ok(())
    }
}
