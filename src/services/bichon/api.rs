use eyre::{Context, Result};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;

const MAX_RETRIES: usize = 3;

/// Bichon 2.x renamed the read-side field to `download_folders` (nullable);
/// 0.3.7 serves `sync_folders`, which also remains the update-payload name.
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub id: u64,
    pub email: String,
    #[serde(
        default,
        alias = "download_folders",
        deserialize_with = "deserialize_null_as_empty"
    )]
    pub sync_folders: Vec<String>,
}

fn deserialize_null_as_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct PaginatedResponse<T> {
    items: Vec<T>,
    #[serde(default)]
    total_pages: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Mailbox {
    pub name: String,
    #[serde(default)]
    pub attributes: Vec<MailboxAttribute>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MailboxAttribute {
    pub attr: String,
    #[serde(default)]
    pub extension: Option<String>,
}

impl MailboxAttribute {
    pub fn kind(&self) -> &str {
        if self.attr == "Extension" {
            self.extension.as_deref().unwrap_or(&self.attr)
        } else {
            &self.attr
        }
    }
}

#[derive(Debug, Serialize)]
struct AccountUpdateRequest<'a> {
    sync_folders: &'a [String],
}

// Bichon caps page_size at 500; anything larger is an InvalidParameter error.
const SEARCH_PAGE_SIZE: u64 = 500;

/// The slice of Bichon's search envelope the coverage comparison reads.
/// `message_id` is Bichon's own field — a header value with angle brackets
/// stripped, or a bracket-kept synthetic id for a message that had none — not
/// the canonical identity the archive sidecars record (ADR-0013).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StoreEnvelope {
    pub message_id: String,
    pub mailbox_name: Option<String>,
    pub date: i64,
    pub uid: u32,
}

#[derive(Debug, Serialize)]
struct SearchFilter {
    account_ids: [u64; 1],
    before: i64,
}

#[derive(Debug, Serialize)]
struct SearchRequest {
    filter: SearchFilter,
    page: u64,
    page_size: u64,
    sort_by: &'static str,
    desc: bool,
}

#[derive(Debug)]
pub struct BichonApiHttpError {
    pub status: StatusCode,
    pub url: String,
    pub body: String,
}

impl std::fmt::Display for BichonApiHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bichon API request failed ({}) {}: {}",
            self.status, self.url, self.body
        )
    }
}

impl std::error::Error for BichonApiHttpError {}

fn is_retryable(err: &eyre::Report) -> bool {
    err.chain()
        .find_map(|e| {
            if let Some(api_err) = e.downcast_ref::<BichonApiHttpError>() {
                return Some(api_err.status);
            }
            if let Some(re) = e.downcast_ref::<reqwest::Error>() {
                return re.status();
            }
            None
        })
        .is_some_and(|status| status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
}

#[derive(Clone)]
pub struct BichonApiClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl BichonApiClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .wrap_err("failed to build Bichon HTTP client")?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http,
        })
    }

    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        let envelope: PaginatedResponse<Account> = self
            .request_json(Method::GET, "/api/v1/accounts", Option::<&()>::None)
            .await?;
        if matches!(envelope.total_pages, Some(n) if n > 1) {
            eyre::bail!(
                "Bichon /api/v1/accounts returned multi-page response (total_pages={:?}); pagination traversal is not implemented",
                envelope.total_pages
            );
        }
        Ok(envelope.items)
    }

    /// Every store envelope of one account whose message Date is <= `before_ms`,
    /// across all folders. Bichon's `before` bound is inclusive; callers pass
    /// `cutoff_midnight_ms - 1` to mean "strictly older than the cutoff date".
    pub async fn search_messages(
        &self,
        account_id: u64,
        before_ms: i64,
    ) -> Result<Vec<StoreEnvelope>> {
        let mut envelopes = Vec::new();
        let mut page = 1u64;
        loop {
            let request = SearchRequest {
                filter: SearchFilter {
                    account_ids: [account_id],
                    before: before_ms,
                },
                page,
                page_size: SEARCH_PAGE_SIZE,
                sort_by: "DATE",
                desc: false,
            };
            let response: PaginatedResponse<StoreEnvelope> = self
                .request_json(Method::POST, "/api/v1/search-messages", Some(&request))
                .await?;
            let total_pages = response.total_pages.unwrap_or(0);
            // An empty page also terminates: total_pages is recomputed per
            // request, and trusting it alone against a shrinking result set
            // would loop on pages that no longer exist.
            if response.items.is_empty() {
                break;
            }
            envelopes.extend(response.items);
            if page >= total_pages {
                break;
            }
            page += 1;
        }
        Ok(envelopes)
    }

    pub async fn list_mailboxes(&self, account_id: u64) -> Result<Vec<Mailbox>> {
        self.request_json(
            Method::GET,
            &format!("/api/v1/list-mailboxes/{account_id}?remote=true"),
            Option::<&()>::None,
        )
        .await
    }

    pub async fn update_account_sync_folders(
        &self,
        account_id: u64,
        sync_folders: &[String],
    ) -> Result<()> {
        let payload = AccountUpdateRequest { sync_folders };
        self.request_empty(
            Method::POST,
            &format!("/api/v1/account/{account_id}"),
            Some(&payload),
        )
        .await
    }

    async fn request_json<Body, Output>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
    ) -> Result<Output>
    where
        Body: Serialize + ?Sized,
        Output: DeserializeOwned,
    {
        self.retry(|| self.request_json_once(method.clone(), path, body))
            .await
    }

    async fn request_empty<Body>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
    ) -> Result<()>
    where
        Body: Serialize + ?Sized,
    {
        self.retry(|| async { self.send_once(method.clone(), path, body).await.map(|_| ()) })
            .await
    }

    async fn retry<T, F, Fut>(&self, mut op: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_err: Option<eyre::Report> = None;
        for attempt in 1..=MAX_RETRIES {
            match op().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if attempt < MAX_RETRIES && is_retryable(&err) {
                        let backoff_ms = attempt as u64 * 200;
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    last_err = Some(err);
                    break;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| eyre::eyre!("unknown Bichon API error")))
    }

    async fn request_json_once<Body, Output>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
    ) -> Result<Output>
    where
        Body: Serialize + ?Sized,
        Output: DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, path);
        let response = self.send_once(method, path, body).await?;
        response
            .json::<Output>()
            .await
            .wrap_err_with(|| format!("failed to parse JSON response from {url}"))
    }

    async fn send_once<Body>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
    ) -> Result<reqwest::Response>
    where
        Body: Serialize + ?Sized,
    {
        let url = format!("{}{}", self.base_url, path);
        let request = self
            .http
            .request(method, &url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json");
        let request = if let Some(payload) = body {
            request.json(payload)
        } else {
            request
        };

        let response = request.send().await.map_err(eyre::Report::new)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(eyre::Report::new(BichonApiHttpError { status, url, body }));
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::{Account, BichonApiClient, BichonApiHttpError, StoreEnvelope, is_retryable};
    use reqwest::StatusCode;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn envelope(message_id: &str, mailbox_name: &str, date: i64, uid: u32) -> serde_json::Value {
        json!({
            "id": format!("env-{uid}"),
            "message_id": message_id,
            "account_id": 1,
            "mailbox_name": mailbox_name,
            "uid": uid,
            "date": date,
            "internal_date": date,
            "subject": "s",
            "from": "f@x.io",
        })
    }

    #[tokio::test]
    async fn search_messages_walks_every_page() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({
                "filter": {"account_ids": [7], "before": 999},
                "page": 1,
                "sort_by": "DATE",
                "desc": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [envelope("a@x.io", "INBOX", 10, 1)],
                "total_pages": 2,
                "total_items": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({"page": 2})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [envelope("<0f.1.2@bichon>", "Sent", 20, 2)],
                "total_pages": 2,
                "total_items": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let envelopes = client.search_messages(7, 999).await.unwrap();

        assert_eq!(
            envelopes,
            vec![
                StoreEnvelope {
                    message_id: "a@x.io".to_string(),
                    mailbox_name: Some("INBOX".to_string()),
                    date: 10,
                    uid: 1,
                },
                StoreEnvelope {
                    message_id: "<0f.1.2@bichon>".to_string(),
                    mailbox_name: Some("Sent".to_string()),
                    date: 20,
                    uid: 2,
                },
            ]
        );
    }

    #[tokio::test]
    async fn search_messages_stops_on_an_empty_window() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [],
                "total_pages": 0,
                "total_items": 0
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let envelopes = client.search_messages(7, 999).await.unwrap();
        assert!(envelopes.is_empty());
    }

    // A page that reports fewer total_pages than already fetched, or repeats
    // empty items, must not loop: the empty-items guard is the terminator.
    #[tokio::test]
    async fn search_messages_terminates_when_items_dry_up_before_total_pages() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({"page": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [envelope("a@x.io", "INBOX", 10, 1)],
                "total_pages": 3,
                "total_items": 3
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({"page": 2})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [],
                "total_pages": 3,
                "total_items": 3
            })))
            .mount(&server)
            .await;

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let envelopes = client.search_messages(7, 999).await.unwrap();
        assert_eq!(envelopes.len(), 1);
    }

    #[test]
    fn account_reads_v2_download_folders() {
        let account: Account = serde_json::from_value(json!({
            "id": 1, "email": "me@x.io", "download_folders": ["INBOX", "Sent"]
        }))
        .unwrap();
        assert_eq!(account.sync_folders, vec!["INBOX", "Sent"]);
    }

    #[test]
    fn account_reads_legacy_sync_folders() {
        let account: Account = serde_json::from_value(json!({
            "id": 1, "email": "me@x.io", "sync_folders": ["INBOX"]
        }))
        .unwrap();
        assert_eq!(account.sync_folders, vec!["INBOX"]);
    }

    #[test]
    fn account_reads_null_download_folders_as_empty() {
        let account: Account = serde_json::from_value(json!({
            "id": 1, "email": "me@x.io", "download_folders": null
        }))
        .unwrap();
        assert!(account.sync_folders.is_empty());
    }

    #[test]
    fn account_reads_missing_folders_as_empty() {
        let account: Account =
            serde_json::from_value(json!({"id": 1, "email": "me@x.io"})).unwrap();
        assert!(account.sync_folders.is_empty());
    }

    #[test]
    fn http_error_5xx_is_retryable() {
        let err = eyre::Report::new(BichonApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            url: "http://example".to_string(),
            body: "boom".to_string(),
        });
        assert!(is_retryable(&err));
    }

    #[test]
    fn http_error_429_is_retryable() {
        let err = eyre::Report::new(BichonApiHttpError {
            status: StatusCode::TOO_MANY_REQUESTS,
            url: "http://example".to_string(),
            body: String::new(),
        });
        assert!(is_retryable(&err));
    }

    #[test]
    fn http_error_4xx_other_is_not_retryable() {
        let err = eyre::Report::new(BichonApiHttpError {
            status: StatusCode::BAD_REQUEST,
            url: "http://example".to_string(),
            body: String::new(),
        });
        assert!(!is_retryable(&err));
    }

    #[test]
    fn http_error_survives_wrap_err() {
        let err = eyre::Report::new(BichonApiHttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            url: "http://example".to_string(),
            body: String::new(),
        })
        .wrap_err("outer context");
        assert!(is_retryable(&err));
    }

    #[test]
    fn arbitrary_eyre_error_is_not_retryable() {
        let err = eyre::eyre!("not an HTTP error");
        assert!(!is_retryable(&err));
    }
}
