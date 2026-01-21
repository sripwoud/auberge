use crate::config::{Config, DnsConfig};
use eyre::Result;
use namecheap::domains_dns::set_hosts::HostRequest;
use namecheap::{Host, NameCheapClient};
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
    pub active_records: Vec<Host>,
    pub missing_subdomains: Vec<String>,
}

pub struct DnsService {
    client: NameCheapClient,
    config: DnsConfig,
}

const KNOWN_APP_SUBDOMAINS: &[&str] = &[
    "BLOCKY",
    "CALIBRE",
    "FRESHRSS",
    "NAVIDROME",
    "RADICALE",
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
    pub fn new_with_production(production_override: Option<bool>) -> Result<Self> {
        let app_config = Config::load()?;

        let api_user = app_config.namecheap.api_user.clone();
        let api_key = env::var("NAMECHEAP_API_KEY")
            .map_err(|_| eyre::eyre!("NAMECHEAP_API_KEY environment variable not set"))?;
        let client_ip = env::var("NAMECHEAP_CLIENT_IP").unwrap_or_else(|_| "0.0.0.0".to_string());
        let username = app_config.user.username.clone();

        let production = production_override
            .or_else(|| {
                env::var("NAMECHEAP_PRODUCTION")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(false);

        let client = NameCheapClient::new(api_user, api_key, client_ip, username, production);

        Ok(Self {
            client,
            config: app_config.dns,
        })
    }

    pub fn is_production(&self) -> bool {
        self.client.production
    }

    pub fn config(&self) -> &DnsConfig {
        &self.config
    }

    pub async fn list_records(&self) -> Result<Vec<Host>> {
        let result = self
            .client
            .domains_dns_get_hosts(self.config.sld(), self.config.tld())
            .await
            .map_err(|e| eyre::eyre!("Failed to get DNS hosts: {}", e))?;

        let hosts: Vec<Host> = match result.as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| {
                    serde_json::from_value(v.clone())
                        .map_err(|e| {
                            eprintln!("Warning: Failed to parse host record: {}", e);
                            eprintln!("Record data: {:#?}", v);
                        })
                        .ok()
                })
                .collect(),
            None => {
                if result.is_object() {
                    vec![serde_json::from_value(result).unwrap_or_else(|_| Host::new())]
                } else {
                    vec![]
                }
            }
        };

        Ok(hosts)
    }

    pub async fn set_a_record(&self, subdomain: &str, ip: &str) -> Result<()> {
        let new_record = HostRequest::new(
            subdomain.to_string(),
            "A".to_string(),
            ip.to_string(),
            None,
            None,
            Some(self.config.default_ttl.to_string()),
            None,
            None,
        );

        self.client
            .domains_dns_set_hosts(self.config.sld(), self.config.tld(), vec![new_record])
            .await
            .map_err(|e| eyre::eyre!("Failed to set DNS hosts: {}", e))?;

        Ok(())
    }

    pub async fn migrate_all(&self, new_ip: &str, dry_run: bool) -> Result<Vec<MigrationResult>> {
        let existing = self.list_records().await?;
        let mut results = Vec::new();

        let a_records: Vec<&Host> = existing.iter().filter(|h| h.type_ == "A").collect();

        if dry_run {
            for record in a_records {
                results.push(MigrationResult {
                    subdomain: record.name.clone(),
                    old_ip: record.address.clone(),
                    new_ip: new_ip.to_string(),
                    success: true,
                });
            }
            return Ok(results);
        }

        let hosts: Vec<HostRequest> = existing
            .iter()
            .map(|h| {
                let address = if h.type_ == "A" {
                    new_ip.to_string()
                } else {
                    h.address.clone()
                };
                HostRequest::new(
                    h.name.clone(),
                    h.type_.clone(),
                    address,
                    if h.mx_pref.is_empty() {
                        None
                    } else {
                        Some(h.mx_pref.clone())
                    },
                    None,
                    Some(h.ttl.to_string()),
                    None,
                    None,
                )
            })
            .collect();

        let api_result = self
            .client
            .domains_dns_set_hosts(self.config.sld(), self.config.tld(), hosts)
            .await;

        let success = api_result.is_ok();

        for record in a_records {
            results.push(MigrationResult {
                subdomain: record.name.clone(),
                old_ip: record.address.clone(),
                new_ip: new_ip.to_string(),
                success,
            });
        }

        api_result.map_err(|e| eyre::eyre!("Failed to migrate DNS records: {}", e))?;
        Ok(results)
    }

    pub async fn status(&self) -> Result<DnsStatus> {
        let active_records = self.list_records().await?;

        let discovered = discover_subdomains();
        let configured_subdomains: Vec<String> = discovered.values().cloned().collect();

        let active_names: std::collections::HashSet<&str> = active_records
            .iter()
            .filter(|h| h.type_ == "A")
            .map(|h| h.name.as_str())
            .collect();

        let missing_subdomains: Vec<String> = configured_subdomains
            .iter()
            .filter(|s| !active_names.contains(s.as_str()))
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
