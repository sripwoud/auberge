use eyre::{Context, Result};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;

const MAX_RETRIES: usize = 3;

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub id: u64,
    pub email: String,
    #[serde(default)]
    pub sync_folders: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MailBox {
    pub name: String,
    #[serde(default)]
    pub attributes: Vec<MailboxAttribute>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MailboxAttribute {
    KindObject { kind: String },
    Raw(String),
}

impl MailboxAttribute {
    pub fn kind(&self) -> &str {
        match self {
            MailboxAttribute::KindObject { kind } => kind,
            MailboxAttribute::Raw(kind) => kind,
        }
    }
}

#[derive(Debug, Serialize)]
struct AccountUpdateRequest<'a> {
    sync_folders: &'a [String],
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
        self.request_json(Method::GET, "/api/v1/accounts", Option::<&()>::None)
            .await
    }

    pub async fn list_mailboxes(&self, account_id: u64) -> Result<Vec<MailBox>> {
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
        self.request_json::<_, serde_json::Value>(
            Method::POST,
            &format!("/api/v1/account/{account_id}"),
            Some(&payload),
        )
        .await
        .map(|_| ())
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
        let mut last_err: Option<eyre::Report> = None;
        for attempt in 1..=MAX_RETRIES {
            match self.request_json_once(method.clone(), path, body).await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    let retry = err
                        .downcast_ref::<reqwest::Error>()
                        .and_then(reqwest::Error::status)
                        .is_some_and(|status| {
                            status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
                        });
                    if attempt < MAX_RETRIES && retry {
                        tokio::time::sleep(Duration::from_millis((attempt * 200) as u64)).await;
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

        let response = request
            .send()
            .await
            .wrap_err_with(|| format!("request failed: {url}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            eyre::bail!("Bichon API request failed ({status}) {url}: {body}");
        }
        response
            .json::<Output>()
            .await
            .wrap_err_with(|| format!("failed to parse JSON response from {url}"))
    }
}
