//! The Cloudflare adapter — the one module in the crate that names a vendor
//! type. Everything above it reads `services::dns`'s own `DnsRecord`, which
//! this module translates into. `tests/vendor_types_stay_in_adapter.rs` fences
//! that boundary.

use crate::config::Config;
use crate::services::dns::{DnsRecord, DnsRecords, RecordContent};
use cloudflare::endpoints::dns::dns::{
    CreateDnsRecord, CreateDnsRecordParams, DeleteDnsRecord, DnsContent, ListDnsRecords,
    UpdateDnsRecord, UpdateDnsRecordParams,
};
use cloudflare::endpoints::zones::zone::{ListZones, ListZonesParams};
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::ClientConfig;
use cloudflare::framework::client::async_api::Client;
use eyre::Result;

/// Translates one record out of the vendor's shape. `DnsContent` is a closed
/// enum in `cloudflare` 0.14, so a new record type upstream breaks this match
/// rather than silently reaching a caller as something else.
fn translate(record: cloudflare::endpoints::dns::dns::DnsRecord) -> DnsRecord {
    let content = match record.content {
        DnsContent::A { content } => RecordContent::A { ip: content },
        DnsContent::AAAA { content } => RecordContent::Aaaa { ip: content },
        DnsContent::CNAME { content } => RecordContent::Cname { target: content },
        DnsContent::MX { content, priority } => RecordContent::Mx {
            target: content,
            priority,
        },
        DnsContent::NS { content } => RecordContent::Ns { target: content },
        DnsContent::SRV { content } => RecordContent::Srv { target: content },
        DnsContent::TXT { content } => RecordContent::Txt { text: content },
    };
    DnsRecord {
        id: record.id,
        name: record.name,
        ttl: record.ttl,
        content,
    }
}

pub struct CloudflareDns {
    client: Client,
    domain: String,
    default_ttl: u32,
    zone_id: String,
}

impl CloudflareDns {
    /// Reads the API token from config, then resolves the Domain to its zone.
    /// Zone discovery is the one network call constructing this makes; it lives
    /// here so nothing above the seam has to make a request to exist.
    pub async fn connect() -> Result<Self> {
        let config = Config::load()?;

        let api_token = config
            .get_resolved("cloudflare_dns_api_token")?
            .filter(|v| !v.is_empty())
            .ok_or_else(|| eyre::eyre!("cloudflare_dns_api_token not set in config"))?;

        let client = Client::new(
            Credentials::UserAuthToken { token: api_token },
            ClientConfig::default(),
            Environment::Production,
        )?;

        let domain = config.domain();
        let zone_id = Self::discover_zone_id(&client, &domain).await?;

        Ok(Self {
            client,
            domain,
            default_ttl: config.ttl(),
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

        let mut results = zones.result;
        match results.len() {
            0 => eyre::bail!("Zone not found: {}", zone_name),
            1 => Ok(results.remove(0).id),
            _ => {
                let ids: Vec<String> = results.iter().map(|z| z.id.clone()).collect();
                eyre::bail!(
                    "Multiple zones found for '{}': {:?}. Scope your API token to a single zone.",
                    zone_name,
                    ids
                )
            }
        }
    }

    /// The zone's A record for `subdomain`. CNAME / AAAA / TXT records sharing
    /// the name are not candidates — every write this adapter makes is an A.
    async fn find_a_record(&self, subdomain: &str) -> Result<Option<DnsRecord>> {
        let full_name = format!("{}.{}", subdomain, self.domain);
        Ok(self
            .list_records()
            .await?
            .into_iter()
            .find(|r| r.name == full_name && r.a_ip().is_some()))
    }
}

impl DnsRecords for CloudflareDns {
    fn domain(&self) -> &str {
        &self.domain
    }

    async fn list_records(&self) -> Result<Vec<DnsRecord>> {
        let response = self
            .client
            .request(&ListDnsRecords {
                zone_identifier: &self.zone_id,
                params: Default::default(),
            })
            .await
            .map_err(|e| eyre::eyre!("Failed to list DNS records: {}", e))?;

        Ok(response.result.into_iter().map(translate).collect())
    }

    async fn set_a_record(&self, subdomain: &str, ip: &str) -> Result<()> {
        let existing = self.find_a_record(subdomain).await?;
        let full_name = format!("{}.{}", subdomain, self.domain);
        let ip_addr = ip
            .parse()
            .map_err(|e| eyre::eyre!("Invalid IP address: {}", e))?;

        match existing {
            Some(record) => {
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
            }
            None => {
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
        }

        Ok(())
    }

    async fn delete_a_record(&self, subdomain: &str) -> Result<bool> {
        let Some(record) = self.find_a_record(subdomain).await? else {
            return Ok(false);
        };
        self.client
            .request(&DeleteDnsRecord {
                zone_identifier: &self.zone_id,
                identifier: &record.id,
            })
            .await
            .map_err(|e| eyre::eyre!("Failed to delete DNS record: {}", e))?;
        Ok(true)
    }
}
