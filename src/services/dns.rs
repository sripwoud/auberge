use crate::config::{Config, DnsConfig};
use cloudflare::endpoints::dns::dns::{
    CreateDnsRecord, CreateDnsRecordParams, DnsContent, DnsRecord, ListDnsRecords, UpdateDnsRecord,
    UpdateDnsRecordParams,
};
use cloudflare::endpoints::zones::zone::{ListZones, ListZonesParams};
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::ClientConfig;
use cloudflare::framework::client::async_api::Client;
use eyre::Result;
use std::collections::HashMap;
use std::env;

pub struct MigrationResult {
    pub subdomain: String,
    pub old_ip: String,
    pub new_ip: String,
    pub success: bool,
}

pub struct DnsStatus {
    pub domain: String,
    pub configured_subdomains: Vec<String>,
    pub active_records: Vec<DnsRecord>,
    pub missing_subdomains: Vec<String>,
}

pub struct DnsService {
    client: Client,
    config: DnsConfig,
    zone_id: String,
}

const KNOWN_APP_SUBDOMAINS: &[&str] = &[
    "BAIKAL",
    "BLOCKY",
    "CALIBRE",
    "FRESHRSS",
    "NAVIDROME",
    "WEBDAV",
    "YOURLS",
];

pub fn discover_subdomains() -> HashMap<String, String> {
    KNOWN_APP_SUBDOMAINS
        .iter()
        .filter_map(|app| {
            let key = format!("{}_SUBDOMAIN", app);
            env::var(&key).ok().map(|value| (app.to_lowercase(), value))
        })
        .collect()
}

impl DnsService {
    pub async fn new_with_production(_production_override: Option<bool>) -> Result<Self> {
        let app_config = Config::load()?;

        let api_token = env::var("CLOUDFLARE_DNS_API_TOKEN")
            .map_err(|_| eyre::eyre!("CLOUDFLARE_DNS_API_TOKEN not set"))?;

        let credentials = Credentials::UserAuthToken { token: api_token };

        let client = Client::new(
            credentials,
            ClientConfig::default(),
            Environment::Production,
        )?;

        let zone_id = match &app_config.cloudflare.zone_id {
            Some(id) => id.clone(),
            None => Self::discover_zone_id(&client, app_config.dns.zone_name()).await?,
        };

        Ok(Self {
            client,
            config: app_config.dns,
            zone_id,
        })
    }

    async fn discover_zone_id(client: &Client, zone_name: &str) -> Result<String> {
        let zones = client
            .request(&ListZones {
                params: ListZonesParams {
                    name: Some(zone_name.to_string()),
                    ..Default::default()
                },
            })
            .await
            .map_err(|e| eyre::eyre!("Failed to list zones: {}", e))?;

        zones
            .result
            .into_iter()
            .next()
            .map(|z| z.id)
            .ok_or_else(|| eyre::eyre!("Zone not found: {}", zone_name))
    }

    pub fn config(&self) -> &DnsConfig {
        &self.config
    }

    pub async fn list_records(&self) -> Result<Vec<DnsRecord>> {
        let response = self
            .client
            .request(&ListDnsRecords {
                zone_identifier: &self.zone_id,
                params: Default::default(),
            })
            .await
            .map_err(|e| eyre::eyre!("Failed to list DNS records: {}", e))?;

        Ok(response.result)
    }

    async fn find_record(&self, subdomain: &str) -> Result<Option<DnsRecord>> {
        let records = self.list_records().await?;
        let full_name = format!("{}.{}", subdomain, self.config.domain);

        Ok(records
            .into_iter()
            .find(|r| r.name == full_name && matches!(r.content, DnsContent::A { .. })))
    }

    pub async fn set_a_record(&self, subdomain: &str, ip: &str) -> Result<()> {
        let existing = self.find_record(subdomain).await?;
        let full_name = format!("{}.{}", subdomain, self.config.domain);
        let ip_addr = ip
            .parse()
            .map_err(|e| eyre::eyre!("Invalid IP address: {}", e))?;

        if let Some(record) = existing {
            self.client
                .request(&UpdateDnsRecord {
                    zone_identifier: &self.zone_id,
                    identifier: &record.id,
                    params: UpdateDnsRecordParams {
                        name: &full_name,
                        content: DnsContent::A { content: ip_addr },
                        ttl: Some(self.config.default_ttl),
                        proxied: Some(false),
                    },
                })
                .await
                .map_err(|e| eyre::eyre!("Failed to update DNS record: {}", e))?;
        } else {
            self.client
                .request(&CreateDnsRecord {
                    zone_identifier: &self.zone_id,
                    params: CreateDnsRecordParams {
                        name: &full_name,
                        content: DnsContent::A { content: ip_addr },
                        ttl: Some(self.config.default_ttl),
                        proxied: Some(false),
                        priority: None,
                    },
                })
                .await
                .map_err(|e| eyre::eyre!("Failed to create DNS record: {}", e))?;
        }

        Ok(())
    }

    pub async fn migrate_all(&self, new_ip: &str, dry_run: bool) -> Result<Vec<MigrationResult>> {
        let existing = self.list_records().await?;
        let mut results = Vec::new();

        let a_records: Vec<&DnsRecord> = existing
            .iter()
            .filter(|r| matches!(r.content, DnsContent::A { .. }))
            .collect();

        if dry_run {
            for record in a_records {
                if let DnsContent::A { content: old_ip } = record.content {
                    let subdomain = record
                        .name
                        .strip_suffix(&format!(".{}", self.config.domain))
                        .unwrap_or(&record.name)
                        .to_string();

                    results.push(MigrationResult {
                        subdomain,
                        old_ip: old_ip.to_string(),
                        new_ip: new_ip.to_string(),
                        success: true,
                    });
                }
            }
            return Ok(results);
        }

        for record in a_records {
            if let DnsContent::A { content: old_ip } = record.content {
                let subdomain = record
                    .name
                    .strip_suffix(&format!(".{}", self.config.domain))
                    .unwrap_or(&record.name);

                let success = self.set_a_record(subdomain, new_ip).await.is_ok();

                results.push(MigrationResult {
                    subdomain: subdomain.to_string(),
                    old_ip: old_ip.to_string(),
                    new_ip: new_ip.to_string(),
                    success,
                });
            }
        }

        Ok(results)
    }

    pub async fn status(&self) -> Result<DnsStatus> {
        let active_records = self.list_records().await?;

        let discovered = discover_subdomains();
        let configured_subdomains: Vec<String> = discovered.values().cloned().collect();

        let active_names: std::collections::HashSet<String> = active_records
            .iter()
            .filter(|r| matches!(r.content, DnsContent::A { .. }))
            .map(|r| {
                r.name
                    .strip_suffix(&format!(".{}", self.config.domain))
                    .unwrap_or(&r.name)
                    .to_string()
            })
            .collect();

        let missing_subdomains: Vec<String> = configured_subdomains
            .iter()
            .filter(|s| !active_names.contains(*s))
            .cloned()
            .collect();

        Ok(DnsStatus {
            domain: self.config.domain.clone(),
            configured_subdomains,
            active_records,
            missing_subdomains,
        })
    }
}
