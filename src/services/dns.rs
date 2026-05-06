use crate::ansible_assets::AnsibleAssets;
use crate::config::Config;
use crate::playbook_meta::PlaybookMeta;
use cloudflare::endpoints::dns::dns::{
    CreateDnsRecord, CreateDnsRecordParams, DeleteDnsRecord, DnsContent, DnsRecord, ListDnsRecords,
    UpdateDnsRecord, UpdateDnsRecordParams,
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

#[derive(Debug)]
pub struct SubdomainEntry {
    pub subdomain: String,
    pub ip_override: Option<String>,
}

#[derive(Default)]
pub struct DiscoveredSubdomains {
    pub public: HashMap<String, SubdomainEntry>,
    pub tailnet_only: HashMap<String, SubdomainEntry>,
}

/// Walks the playbooks directory once and returns App subdomains partitioned
/// by ADR-0003 publication channel:
/// - `public`     — Cloudflare A records (subject to per-app `_tailscale_ip` override)
/// - `tailnet_only` — Blocky `customDNS` map (no Cloudflare A record ever)
///
/// Metas with an empty/missing `subdomain` are silently dropped; the integrity
/// test in this module (`test_every_app_meta_has_subdomain_unless_tailnet_only_or_excluded`)
/// enforces that App metas declare a non-empty `subdomain`, so the silent drop
/// is unreachable for in-tree metas.
pub fn discover_all_subdomains() -> DiscoveredSubdomains {
    let assets = match AnsibleAssets::prepare() {
        Ok(a) => a,
        Err(_) => return DiscoveredSubdomains::default(),
    };
    let entries = match std::fs::read_dir(assets.playbooks_dir()) {
        Ok(e) => e,
        Err(_) => return DiscoveredSubdomains::default(),
    };

    let config = Config::load().ok();
    let mut public: HashMap<String, SubdomainEntry> = HashMap::new();
    let mut tailnet_only: HashMap<String, SubdomainEntry> = HashMap::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(app) = file_name.strip_suffix(".meta.yml") else {
            continue;
        };
        let Ok(meta) = PlaybookMeta::load(&path) else {
            continue;
        };
        let Some(subdomain) = meta.subdomain.clone().filter(|s| !s.is_empty()) else {
            continue;
        };

        if meta.tailnet_only {
            tailnet_only.insert(
                app.to_string(),
                SubdomainEntry {
                    subdomain,
                    ip_override: None,
                },
            );
        } else {
            let ip_override = config.as_ref().and_then(|c| {
                let key = format!("{}_tailscale_ip", app);
                c.get(&key).filter(|v| !v.is_empty())
            });
            public.insert(
                app.to_string(),
                SubdomainEntry {
                    subdomain,
                    ip_override,
                },
            );
        }
    }

    DiscoveredSubdomains {
        public,
        tailnet_only,
    }
}

/// Public-App subdomains only. Thin wrapper for callers that don't need the
/// tailnet-only half (status, interactive subdomain pickers, etc.).
pub fn discover_subdomains() -> HashMap<String, SubdomainEntry> {
    discover_all_subdomains().public
}

/// Returns `true` if `ip` is in the Tailscale CGNAT range (100.64.0.0/10).
pub fn is_tailscale_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let octets = addr.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

impl DnsService {
    pub async fn new_with_production(_production_override: Option<bool>) -> Result<Self> {
        let config = Config::load()?;

        let api_token = config
            .get_resolved("cloudflare_dns_api_token")?
            .filter(|v| !v.is_empty())
            .ok_or_else(|| eyre::eyre!("cloudflare_dns_api_token not set in config"))?;

        let credentials = Credentials::UserAuthToken { token: api_token };

        let client = Client::new(
            credentials,
            ClientConfig::default(),
            Environment::Production,
        )?;

        let domain = config.domain();
        let default_ttl = config.ttl();
        let zone_id = Self::discover_zone_id(&client, &domain).await?;

        Ok(Self {
            client,
            domain,
            default_ttl,
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

    /// Deletes the A record for `subdomain`.  Returns `true` when the record
    /// was found and deleted, `false` when it was already absent (idempotent).
    pub async fn delete_a_record(&self, subdomain: &str) -> Result<bool> {
        let existing = self.find_record(subdomain).await?;
        match existing {
            None => Ok(false),
            Some(record) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn test_is_tailscale_ip_true() {
        assert!(is_tailscale_ip("100.64.0.1"));
        assert!(is_tailscale_ip("100.100.200.1"));
        assert!(is_tailscale_ip("100.127.255.255"));
    }

    #[test]
    fn test_is_tailscale_ip_false() {
        assert!(!is_tailscale_ip("100.128.0.1"));
        assert!(!is_tailscale_ip("192.168.1.1"));
        assert!(!is_tailscale_ip("203.0.113.10"));
        assert!(!is_tailscale_ip("not-an-ip"));
    }

    fn playbooks_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("playbooks")
    }

    // Metas for orchestrating/infrastructure playbooks that don't represent
    // an App with DNS publication. Anything else must declare `subdomain`.
    const NON_APP_PLAYBOOK_METAS: &[&str] = &[
        "apps",
        "bootstrap",
        "calibre",
        "hardening",
        "hermes",
        "infrastructure",
        "remove-radicale",
        "vibecoder",
    ];

    #[test]
    fn test_every_app_meta_has_subdomain_unless_tailnet_only_or_excluded() {
        let dir = playbooks_dir();
        let read = std::fs::read_dir(&dir).expect("playbooks dir");
        for entry in read.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".meta.yml") else {
                continue;
            };
            let meta =
                PlaybookMeta::load(&path).unwrap_or_else(|e| panic!("failed to parse {name}: {e}"));

            if meta.tailnet_only {
                assert!(
                    meta.subdomain.as_deref().is_some_and(|s| !s.is_empty()),
                    "{name}: tailnet_only metas must declare subdomain"
                );
                continue;
            }

            if NON_APP_PLAYBOOK_METAS.contains(&stem) {
                continue;
            }

            assert!(
                meta.subdomain.as_deref().is_some_and(|s| !s.is_empty()),
                "{name}: non-tailnet-only App meta must declare subdomain. \
                 If this Playbook Meta does not represent an App, add its stem \
                 to NON_APP_PLAYBOOK_METAS in src/services/dns.rs tests."
            );
        }
    }

    #[test]
    fn test_discover_subdomains_returns_expected_public_apps() {
        let discovered = discover_subdomains();
        let got: BTreeSet<String> = discovered.keys().cloned().collect();
        let expected: BTreeSet<String> = [
            "baikal",
            "blocky",
            "colporteur",
            "freshrss",
            "grimmory",
            "headscale",
            "navidrome",
            "webdav",
            "yourls",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn test_discover_subdomains_excludes_tailnet_only_apps() {
        let discovered = discover_subdomains();
        for tailnet_only in ["bichon", "cockpit", "paperless"] {
            assert!(
                !discovered.contains_key(tailnet_only),
                "tailnet-only app {tailnet_only} must not appear in Public-App discovery"
            );
        }
    }

    #[test]
    fn test_discover_subdomains_uses_meta_subdomain_value() {
        let discovered = discover_subdomains();
        assert_eq!(discovered["headscale"].subdomain, "hs");
        assert_eq!(discovered["freshrss"].subdomain, "freshrss");
    }
}
