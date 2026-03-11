use crate::config::Config;
use crate::user_config::UserConfig;
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
    domain: String,
    default_ttl: u32,
    zone_id: String,
}

pub struct SubdomainEntry {
    pub subdomain: String,
    pub ip_override: Option<String>,
}

const KNOWN_SUBDOMAIN_KEYS: &[&str] = &[
    "baikal_subdomain",
    "bichon_subdomain",
    "blocky_subdomain",
    "booklore_subdomain",
    "colporteur_subdomain",
    "freshrss_subdomain",
    "headscale_subdomain",
    "navidrome_subdomain",
    "paperless_subdomain",
    "webdav_subdomain",
    "yourls_subdomain",
];

pub fn discover_subdomains() -> HashMap<String, SubdomainEntry> {
    let config = match UserConfig::load() {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    KNOWN_SUBDOMAIN_KEYS
        .iter()
        .filter_map(|key| {
            config.get(key).filter(|v| !v.is_empty()).map(|value| {
                let app = key.strip_suffix("_subdomain").unwrap_or(key);
                let tailscale_key = format!("{}_tailscale_ip", app);
                let ip_override = config.get(&tailscale_key).filter(|v| !v.is_empty());
                (
                    app.to_string(),
                    SubdomainEntry {
                        subdomain: value,
                        ip_override,
                    },
                )
            })
        })
        .collect()
}

fn is_tailscale_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let octets = addr.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

impl DnsService {
    pub async fn new_with_production(_production_override: Option<bool>) -> Result<Self> {
        let app_config = Config::load()?;
        let user_config = UserConfig::load()?;

        let api_token = user_config
            .get("cloudflare_dns_api_token")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| eyre::eyre!("cloudflare_dns_api_token not set in config"))?;

        let credentials = Credentials::UserAuthToken { token: api_token };

        let client = Client::new(
            credentials,
            ClientConfig::default(),
            Environment::Production,
        )?;

        let zone_id = Self::discover_zone_id(&client, &app_config.domain).await?;

        Ok(Self {
            client,
            domain: app_config.domain,
            default_ttl: app_config.default_ttl,
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

    pub fn domain(&self) -> &str {
        &self.domain
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
        let full_name = format!("{}.{}", subdomain, self.domain);

        Ok(records
            .into_iter()
            .find(|r| r.name == full_name && matches!(r.content, DnsContent::A { .. })))
    }

    pub async fn set_a_record(&self, subdomain: &str, ip: &str) -> Result<()> {
        let existing = self.find_record(subdomain).await?;
        let full_name = format!("{}.{}", subdomain, self.domain);
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
                        ttl: Some(self.default_ttl),
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
                        ttl: Some(self.default_ttl),
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

        let domain_suffix = format!(".{}", self.domain);
        let a_records: Vec<&DnsRecord> = existing
            .iter()
            .filter(|r| matches!(r.content, DnsContent::A { .. }))
            .filter(|r| r.name.ends_with(&domain_suffix) && r.name != self.domain)
            .collect();

        if dry_run {
            for record in a_records {
                if let DnsContent::A { content: old_ip } = record.content {
                    if is_tailscale_ip(&old_ip.to_string()) {
                        eprintln!("Skipping tailnet-only record: {}", record.name);
                        continue;
                    }
                    let subdomain = record
                        .name
                        .strip_suffix(&domain_suffix)
                        .expect("pre-filtered to end with domain suffix")
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
                if is_tailscale_ip(&old_ip.to_string()) {
                    eprintln!("Skipping tailnet-only record: {}", record.name);
                    continue;
                }
                let subdomain = record
                    .name
                    .strip_suffix(&domain_suffix)
                    .expect("pre-filtered to end with domain suffix");

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
        let configured_subdomains: Vec<String> =
            discovered.values().map(|e| e.subdomain.clone()).collect();

        let domain_suffix = format!(".{}", self.domain);
        let active_names: std::collections::HashSet<String> = active_records
            .iter()
            .filter(|r| matches!(r.content, DnsContent::A { .. }))
            .map(|r| {
                r.name
                    .strip_suffix(&domain_suffix)
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
            domain: self.domain.clone(),
            configured_subdomains,
            active_records,
            missing_subdomains,
        })
    }
}
