use crate::models::dns::DnsConfig;
use crate::services::inventory::find_project_root;
use eyre::{Result, WrapErr};
use namecheap::domains_dns::set_hosts::HostRequest;
use namecheap::{Host, NameCheapClient};
use std::path::Path;

pub fn load_dns_config(config_path: Option<&Path>) -> Result<DnsConfig> {
    let path = match config_path {
        Some(p) => p.to_path_buf(),
        None => find_project_root().join("inventory/dns.yml"),
    };

    let content = std::fs::read_to_string(&path)
        .wrap_err_with(|| format!("Failed to read {}", path.display()))?;

    let config: DnsConfig = serde_yaml::from_str(&content)
        .wrap_err_with(|| format!("Failed to parse {}", path.display()))?;

    Ok(config)
}

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

impl DnsService {
    pub fn new() -> Result<Self> {
        let client = NameCheapClient::new_from_env()
            .map_err(|e| eyre::eyre!("Failed to create NameCheap client: {}", e))?;
        let config = load_dns_config(None)?;
        Ok(Self { client, config })
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
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
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
        let existing = self.list_records().await?;

        let mut hosts: Vec<HostRequest> = existing
            .iter()
            .filter(|h| !(h.name == subdomain && h.type_ == "A"))
            .map(|h| {
                HostRequest::new(
                    h.name.clone(),
                    h.type_.clone(),
                    h.address.clone(),
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

        hosts.push(HostRequest::new(
            subdomain.to_string(),
            "A".to_string(),
            ip.to_string(),
            None,
            None,
            Some(self.config.default_ttl.to_string()),
            None,
            None,
        ));

        self.client
            .domains_dns_set_hosts(self.config.sld(), self.config.tld(), hosts)
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

        let active_names: std::collections::HashSet<&str> = active_records
            .iter()
            .filter(|h| h.type_ == "A")
            .map(|h| h.name.as_str())
            .collect();

        let missing_subdomains: Vec<String> = self
            .config
            .subdomains
            .iter()
            .filter(|s| !active_names.contains(s.as_str()))
            .cloned()
            .collect();

        Ok(DnsStatus {
            domain: self.config.domain.clone(),
            configured_subdomains: self.config.subdomains.clone(),
            active_records,
            missing_subdomains,
        })
    }
}
