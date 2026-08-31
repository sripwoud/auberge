use crate::ansible_assets::AnsibleAssets;
use crate::config::Config;
use crate::playbook_meta::PlaybookMeta;
use eyre::Result;
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

/// How long `set_all` waits between writes, to stay inside the provider's
/// rate limit. Passed in rather than baked into the loop so a test can apply
/// a plan without sleeping through it.
pub const WRITE_PACE: Duration = Duration::from_millis(500);

/// The value a record publishes, translated out of whatever the provider
/// returned. Nothing in the crate reads a vendor enum to get at an IP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordContent {
    A { ip: Ipv4Addr },
    Aaaa { ip: Ipv6Addr },
    Cname { target: String },
    Mx { target: String, priority: u16 },
    Ns { target: String },
    Srv { target: String },
    Txt { text: String },
}

impl RecordContent {
    /// The record type as DNS spells it — the `TYPE` column of `dns list`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::A { .. } => "A",
            Self::Aaaa { .. } => "AAAA",
            Self::Cname { .. } => "CNAME",
            Self::Mx { .. } => "MX",
            Self::Ns { .. } => "NS",
            Self::Srv { .. } => "SRV",
            Self::Txt { .. } => "TXT",
        }
    }

    /// The record's value as `dns list` renders it.
    pub fn value(&self) -> String {
        match self {
            Self::A { ip } => ip.to_string(),
            Self::Aaaa { ip } => ip.to_string(),
            Self::Cname { target } | Self::Ns { target } | Self::Srv { target } => target.clone(),
            Self::Mx { target, priority } => format!("{} ({})", target, priority),
            Self::Txt { text } => text.clone(),
        }
    }
}

/// One record in the zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsRecord {
    pub id: String,
    pub name: String,
    pub ttl: u32,
    pub content: RecordContent,
}

impl DnsRecord {
    /// The IPv4 address this record publishes, `None` when it is not an A record.
    pub fn a_ip(&self) -> Option<Ipv4Addr> {
        match self.content {
            RecordContent::A { ip } => Some(ip),
            _ => None,
        }
    }
}

/// Record operations against the zone for one Domain.
///
/// Constructing an implementation is where the provider handshake lives — zone
/// discovery included — so every method here is a plain read or write and the
/// plan-and-apply logic below runs against a fake with no network. `DnsLookup`
/// in `dns_verify` is the sibling seam on the query side.
///
/// `async fn` in a trait is fine here: the crate is a binary, these futures are
/// awaited in place from `main`, and nothing needs a `Send` bound on them.
#[allow(async_fn_in_trait)]
pub trait DnsRecords {
    /// The zone's apex domain. Every record name is `<subdomain>.<domain>`.
    fn domain(&self) -> &str;

    async fn list_records(&self) -> Result<Vec<DnsRecord>>;

    /// Creates the A record, or updates it in place when one already exists.
    async fn set_a_record(&self, subdomain: &str, ip: &str) -> Result<()>;

    /// Deletes the A record for `subdomain`. Returns `true` when the record was
    /// found and deleted, `false` when it was already absent (idempotent).
    async fn delete_a_record(&self, subdomain: &str) -> Result<bool>;
}

/// One in-scope A record and the outcome of its write. `success` is `true` on a
/// dry run, which writes nothing.
#[derive(Debug)]
pub struct MigrationResult {
    pub subdomain: String,
    pub old_ip: String,
    pub new_ip: String,
    pub success: bool,
}

/// An A record `migrate` left where it was, and why.
///
/// Keyed on the record, not on an App as `SkippedApp` is: `migrate` reads the
/// zone, so a subdomain here need not name an App at all.
#[derive(Debug, PartialEq, Eq)]
pub struct SkippedRecord {
    pub subdomain: String,
    /// The address the record keeps — the tailnet value that decided the skip.
    pub ip: String,
    pub reason: SkipReason,
}

/// What `migrate_all` did to the zone. `migrated` and `skipped` partition the
/// records the run took as candidates — so a caller diffing against the zone
/// can tell a record left behind on purpose from one the run never saw — and
/// records that were never candidates are in neither.
#[derive(Debug, Default)]
pub struct MigrationOutcome {
    pub migrated: Vec<MigrationResult>,
    pub skipped: Vec<SkippedRecord>,
}

pub struct DnsStatus {
    pub domain: String,
    pub configured_subdomains: Vec<String>,
    pub active_records: Vec<DnsRecord>,
    pub missing_subdomains: Vec<String>,
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
/// - `public`     — Cloudflare A records
/// - `tailnet_only` — Blocky `customDNS` map (no Cloudflare A record ever)
///
/// Both channels read the same per-App `<app>_tailscale_ip` override into
/// `ip_override`, because both publish an address and neither one's address is
/// a property of the Host the publisher runs on (ADR-0059). The tailnet-only
/// half hardcoded `None` while only one Host existed, which read as "a
/// tailnet-only App has no address of its own" rather than as the assumption
/// it was.
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
        let config_override = config.as_ref().and_then(|c| {
            let key = format!("{}_subdomain", app);
            c.get(&key).filter(|v| !v.is_empty())
        });
        let Some(subdomain) =
            config_override.or_else(|| meta.subdomain.clone().filter(|s| !s.is_empty()))
        else {
            continue;
        };

        let ip_override = config.as_ref().and_then(|c| {
            let key = format!("{}_tailscale_ip", app);
            c.get(&key).filter(|v| !v.is_empty())
        });
        let channel = if meta.tailnet_only {
            &mut tailnet_only
        } else {
            &mut public
        };
        channel.insert(
            app.to_string(),
            SubdomainEntry {
                subdomain,
                ip_override,
            },
        );
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

/// Why a record is neither written nor repointed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// ADR-0003 keeps Tailnet-only Apps off Cloudflare, which each subcommand
    /// meets from its own side: `set-all` never creates the record, and
    /// `migrate` never repoints one that already holds a tailnet address.
    TailnetOnly,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TailnetOnly => "tailnet_only",
        }
    }
}

/// One A record `set-all` will write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRecord {
    pub app: String,
    pub subdomain: String,
    pub fqdn: String,
    pub ip: String,
}

/// An App `set-all` will leave alone, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedApp {
    pub app: String,
    pub subdomain: String,
    pub reason: SkipReason,
}

/// Everything `set-all` will do, decided before the first write.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SetAllPlan {
    pub to_create: Vec<PlannedRecord>,
    pub skipped: Vec<SkippedApp>,
}

/// The result of one write in the plan. `error` is `None` on success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedRecord {
    pub subdomain: String,
    pub fqdn: String,
    pub ip: String,
    pub error: Option<String>,
}

/// What applying a plan actually did. `created`, `failed` and `not_attempted`
/// partition the plan: every record it held is in exactly one of them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SetAllOutcome {
    pub created: Vec<AppliedRecord>,
    pub failed: Vec<AppliedRecord>,
    /// Records the run abandoned — fail-fast stopped at an earlier failure
    /// before reaching these. Not write failures, and not counted as such by
    /// the exit status; but a report that named only what it attempted would
    /// drop them from a plan the reader cannot see, so they are still
    /// accounted for as unsuccessful in the ADR-0004 body.
    pub not_attempted: Vec<PlannedRecord>,
}

/// Decides which A records `set-all` writes and which Apps it skips, applying
/// `--subdomains` / `--skip` operator intent and the ADR-0003 invariants:
///
/// - Implicit (`subdomains` empty): every `--skip`-filtered tailnet-only App is
///   skipped; the remaining Public Apps are scheduled to create.
/// - Explicit (`subdomains` non-empty): if any non-`--skip`-excluded entry names
///   a tailnet-only App, hard-error before any provider call. `skipped` is empty
///   in this branch — explicit means the operator owns the choice.
///
/// Both lists come back sorted by App name so output is deterministic
/// regardless of `HashMap` iteration order. A named App the discovery does not
/// know is silently dropped.
pub fn plan_set_all(
    domain: &str,
    target_ip: &str,
    discovered: DiscoveredSubdomains,
    subdomains: Vec<String>,
    skip: &HashSet<String>,
) -> Result<SetAllPlan> {
    let DiscoveredSubdomains {
        mut public,
        tailnet_only,
    } = discovered;

    let (selected, skipped): (Vec<(String, SubdomainEntry)>, Vec<SkippedApp>) =
        if subdomains.is_empty() {
            let mut skipped: Vec<SkippedApp> = tailnet_only
                .into_iter()
                .filter(|(app, _)| !skip.contains(app))
                .map(|(app, entry)| SkippedApp {
                    app,
                    subdomain: entry.subdomain,
                    reason: SkipReason::TailnetOnly,
                })
                .collect();
            skipped.sort_by(|a, b| a.app.cmp(&b.app));

            let selected = public
                .into_iter()
                .filter(|(app, _)| !skip.contains(app))
                .collect();
            (selected, skipped)
        } else {
            let offenders: Vec<String> = subdomains
                .iter()
                .filter(|app| !skip.contains(*app))
                .filter_map(|app| {
                    tailnet_only
                        .get(app)
                        .map(|entry| format!("  • {} (subdomain: {})", app, entry.subdomain))
                })
                .collect();
            if !offenders.is_empty() {
                eyre::bail!(
                    "tailnet-only apps cannot have Cloudflare A records (ADR-0003):\n{}\n\n\
                 DNS for tailnet-only apps is published via Blocky on `auberge deploy <app>`.",
                    offenders.join("\n")
                );
            }
            let selected = subdomains
                .into_iter()
                .filter(|app| !skip.contains(app))
                .filter_map(|app| public.remove(&app).map(|entry| (app, entry)))
                .collect();
            (selected, vec![])
        };

    let mut to_create: Vec<PlannedRecord> = selected
        .into_iter()
        .map(|(app, entry)| PlannedRecord {
            fqdn: format!("{}.{}", entry.subdomain, domain),
            ip: entry.ip_override.unwrap_or_else(|| target_ip.to_string()),
            subdomain: entry.subdomain,
            app,
        })
        .collect();
    to_create.sort_by(|a, b| a.app.cmp(&b.app));

    Ok(SetAllPlan { to_create, skipped })
}

/// Writes every record in `plan`, reporting each one to `report` as it lands so
/// a paced run stays legible. A write failure is recorded, never propagated:
/// the caller reads the verdict off the returned `failed` list, so the failure
/// path is observable in both `--output` modes.
///
/// `continue_on_error` decides whether the first failure stops the run. When it
/// does, the records the run never reached are returned in `not_attempted`, so
/// the outcome still accounts for the whole plan.
///
/// `pace` is the wait between writes — `WRITE_PACE` in production.
pub async fn apply_set_all<D: DnsRecords>(
    dns: &D,
    plan: &SetAllPlan,
    continue_on_error: bool,
    pace: Duration,
    report: &mut dyn FnMut(&AppliedRecord),
) -> SetAllOutcome {
    let mut outcome = SetAllOutcome::default();

    for (idx, planned) in plan.to_create.iter().enumerate() {
        let error = dns
            .set_a_record(&planned.subdomain, &planned.ip)
            .await
            .err()
            .map(|e| e.to_string());
        let applied = AppliedRecord {
            subdomain: planned.subdomain.clone(),
            fqdn: planned.fqdn.clone(),
            ip: planned.ip.clone(),
            error,
        };
        report(&applied);

        if applied.error.is_some() {
            outcome.failed.push(applied);
            if !continue_on_error {
                outcome.not_attempted = plan.to_create[idx + 1..].to_vec();
                break;
            }
        } else {
            outcome.created.push(applied);
        }

        if idx + 1 < plan.to_create.len() {
            tokio::time::sleep(pace).await;
        }
    }

    outcome
}

/// Repoints every non-tailnet A record under the Domain at `new_ip`. Records
/// already holding a Tailscale CGNAT address are left alone — they are a
/// Tailnet-only App's publication, not a Host address — and are returned in
/// `skipped` rather than announced, so the command owns what the human sees.
///
/// Records outside the run's scope (the apex, other domains, non-A types) were
/// never candidates and are in neither list.
pub async fn migrate_all<D: DnsRecords>(
    dns: &D,
    new_ip: &str,
    dry_run: bool,
) -> Result<MigrationOutcome> {
    let domain = dns.domain().to_string();
    let domain_suffix = format!(".{}", domain);
    let records = dns.list_records().await?;
    let mut outcome = MigrationOutcome::default();

    for record in records {
        let Some(old_ip) = record.a_ip() else {
            continue;
        };
        let Some(subdomain) = record.name.strip_suffix(&domain_suffix) else {
            continue;
        };
        if is_tailscale_ip(&old_ip.to_string()) {
            outcome.skipped.push(SkippedRecord {
                subdomain: subdomain.to_string(),
                ip: old_ip.to_string(),
                reason: SkipReason::TailnetOnly,
            });
            continue;
        }

        let success = dry_run || dns.set_a_record(subdomain, new_ip).await.is_ok();
        outcome.migrated.push(MigrationResult {
            subdomain: subdomain.to_string(),
            old_ip: old_ip.to_string(),
            new_ip: new_ip.to_string(),
            success,
        });
    }

    Ok(outcome)
}

/// Reads the zone and reports which of `configured_subdomains` have no A record.
pub async fn status<D: DnsRecords>(
    dns: &D,
    configured_subdomains: Vec<String>,
) -> Result<DnsStatus> {
    let active_records = dns.list_records().await?;
    let domain_suffix = format!(".{}", dns.domain());

    let active_names: HashSet<&str> = active_records
        .iter()
        .filter(|r| r.a_ip().is_some())
        .map(|r| r.name.strip_suffix(&domain_suffix).unwrap_or(&r.name))
        .collect();

    let missing_subdomains: Vec<String> = configured_subdomains
        .iter()
        .filter(|s| !active_names.contains(s.as_str()))
        .cloned()
        .collect();

    Ok(DnsStatus {
        domain: dns.domain().to_string(),
        configured_subdomains,
        active_records,
        missing_subdomains,
    })
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
        "caddy",
        "calibre",
        "hardening",
        "hermes",
        "infrastructure",
        "remove-radicale",
        "syncthing",
        "tgtg",
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
        // discover_subdomains -> Config::load() reads XDG_CONFIG_HOME, which
        // other tests in this binary mutate. Hold TEST_LOCK to serialize.
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let discovered = discover_subdomains();
        let got: BTreeSet<String> = discovered.keys().cloned().collect();
        let expected: BTreeSet<String> = [
            "baikal",
            "blocky",
            "colporteur",
            "freshrss",
            "gokapi",
            "grimmory",
            "headscale",
            "immich",
            "navidrome",
            "radio",
            "yourls",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn test_discover_subdomains_excludes_tailnet_only_apps() {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
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
        use crate::output::EnvVarGuard;
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        // Pin XDG_CONFIG_HOME at an empty dir so no <app>_subdomain
        // override exists and meta defaults are the sole signal.
        let dir = tempfile::tempdir().unwrap();
        let _xdg = EnvVarGuard::set("XDG_CONFIG_HOME", dir.path());
        let discovered = discover_subdomains();
        assert_eq!(discovered["headscale"].subdomain, "hs");
        assert_eq!(discovered["freshrss"].subdomain, "freshrss");
    }

    #[test]
    fn test_discover_subdomains_honors_app_subdomain_config_override() {
        use crate::output::EnvVarGuard;
        let _guard = crate::output::TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            "domain = \"example.com\"\nfreshrss_subdomain = \"news\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let _xdg = EnvVarGuard::set("XDG_CONFIG_HOME", dir.path());

        let discovered = discover_subdomains();
        assert_eq!(
            discovered["freshrss"].subdomain, "news",
            "freshrss_subdomain in config must override meta default"
        );
        assert_eq!(
            discovered["baikal"].subdomain, "baikal",
            "apps without a config override keep their meta default"
        );
    }

    // The ADR-0003 partition read off the live playbook tree, not a fixture:
    // a meta that loses `tailnet_only` would otherwise only surface as a
    // Cloudflare A record published for an App that must not have one.
    #[test]
    fn discover_all_subdomains_partitions_tailnet_only() {
        // discover_all_subdomains -> Config::load() reads XDG_CONFIG_HOME, which
        // other tests in this binary mutate. Hold TEST_LOCK to serialize.
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let discovered = discover_all_subdomains();
        for app in ["bichon", "cockpit", "paperless"] {
            assert!(
                discovered.tailnet_only.contains_key(app),
                "tailnet-only app '{app}' must appear in tailnet-only partition"
            );
            assert!(
                !discovered.public.contains_key(app),
                "tailnet-only app '{app}' must not appear in public partition"
            );
        }
        for app in ["freshrss", "baikal", "navidrome"] {
            assert!(
                discovered.public.contains_key(app),
                "public app '{app}' must appear in public partition"
            );
            assert!(
                !discovered.tailnet_only.contains_key(app),
                "public app '{app}' must not appear in tailnet-only partition"
            );
        }
    }

    // The defect #755 closes, read off the live tree: a Tailnet-only App's
    // address is the App's, not the Blocky Host's. Asserted through the same
    // config key the Public half reads, so the two channels cannot drift into
    // disagreeing about what `<app>_tailscale_ip` means.
    #[test]
    fn discover_all_subdomains_honors_a_tailnet_only_apps_ip_override() {
        use crate::output::EnvVarGuard;
        let _guard = crate::output::TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("auberge/config.toml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            "domain = \"example.com\"\npaperless_tailscale_ip = \"100.64.0.9\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let _xdg = EnvVarGuard::set("XDG_CONFIG_HOME", dir.path());

        let discovered = discover_all_subdomains();
        assert_eq!(
            discovered.tailnet_only["paperless"].ip_override.as_deref(),
            Some("100.64.0.9"),
            "a tailnet-only App declaring `<app>_tailscale_ip` publishes that address"
        );
        assert_eq!(
            discovered.tailnet_only["bichon"].ip_override, None,
            "a tailnet-only App declaring no override carries none"
        );
    }

    // ── Crate-local record types ──────────────────────────────────────────────

    fn a_record(name: &str, ip: &str) -> DnsRecord {
        DnsRecord {
            id: format!("id-{name}"),
            name: name.to_string(),
            ttl: 1,
            content: RecordContent::A {
                ip: ip.parse().unwrap(),
            },
        }
    }

    #[test]
    fn record_content_kind_names_every_variant() {
        let kinds: Vec<&str> = [
            RecordContent::A {
                ip: "1.2.3.4".parse().unwrap(),
            },
            RecordContent::Aaaa {
                ip: "::1".parse().unwrap(),
            },
            RecordContent::Cname {
                target: "a.example.com".to_string(),
            },
            RecordContent::Mx {
                target: "mail.example.com".to_string(),
                priority: 10,
            },
            RecordContent::Ns {
                target: "ns1.example.com".to_string(),
            },
            RecordContent::Srv {
                target: "srv.example.com".to_string(),
            },
            RecordContent::Txt {
                text: "v=spf1 -all".to_string(),
            },
        ]
        .iter()
        .map(|c| c.kind())
        .collect();
        assert_eq!(kinds, ["A", "AAAA", "CNAME", "MX", "NS", "SRV", "TXT"]);
    }

    // `dns list`'s CONTENT column. MX is the only variant that renders more
    // than the bare target, and the `(priority)` suffix is what a reader needs
    // to tell two MX rows apart.
    #[test]
    fn record_content_value_renders_mx_priority() {
        assert_eq!(
            RecordContent::Mx {
                target: "mail.example.com".to_string(),
                priority: 10,
            }
            .value(),
            "mail.example.com (10)"
        );
        assert_eq!(
            RecordContent::A {
                ip: "1.2.3.4".parse().unwrap()
            }
            .value(),
            "1.2.3.4"
        );
    }

    #[test]
    fn a_ip_reads_only_a_records() {
        assert_eq!(
            a_record("rss.example.com", "1.2.3.4").a_ip(),
            Some("1.2.3.4".parse().unwrap())
        );
        let cname = DnsRecord {
            id: "id".to_string(),
            name: "www.example.com".to_string(),
            ttl: 1,
            content: RecordContent::Cname {
                target: "example.com".to_string(),
            },
        };
        assert_eq!(cname.a_ip(), None);
    }

    // ── Fake seam ─────────────────────────────────────────────────────────────

    /// A `DnsRecords` with no network behind it: `records` is what the zone
    /// holds, `fail_on` names subdomains whose write errors, and `writes`
    /// records every write attempt in order.
    struct FakeDns {
        domain: String,
        records: Vec<DnsRecord>,
        fail_on: HashSet<String>,
        writes: std::cell::RefCell<Vec<(String, String)>>,
    }

    impl FakeDns {
        fn new(domain: &str, records: Vec<DnsRecord>) -> Self {
            Self {
                domain: domain.to_string(),
                records,
                fail_on: HashSet::new(),
                writes: std::cell::RefCell::new(vec![]),
            }
        }

        fn failing_on(mut self, subdomains: &[&str]) -> Self {
            self.fail_on = subdomains.iter().map(|s| (*s).to_string()).collect();
            self
        }

        fn writes(&self) -> Vec<(String, String)> {
            self.writes.borrow().clone()
        }
    }

    impl DnsRecords for FakeDns {
        fn domain(&self) -> &str {
            &self.domain
        }

        async fn list_records(&self) -> Result<Vec<DnsRecord>> {
            Ok(self.records.clone())
        }

        async fn set_a_record(&self, subdomain: &str, ip: &str) -> Result<()> {
            self.writes
                .borrow_mut()
                .push((subdomain.to_string(), ip.to_string()));
            if self.fail_on.contains(subdomain) {
                eyre::bail!("cloudflare rejected {subdomain}");
            }
            Ok(())
        }

        async fn delete_a_record(&self, subdomain: &str) -> Result<bool> {
            Ok(self
                .records
                .iter()
                .any(|r| r.name == format!("{}.{}", subdomain, self.domain) && r.a_ip().is_some()))
        }
    }

    // ── plan_set_all ──────────────────────────────────────────────────────────

    fn entry(subdomain: &str) -> SubdomainEntry {
        SubdomainEntry {
            subdomain: subdomain.to_string(),
            ip_override: None,
        }
    }

    fn discovered(
        public: &[(&str, SubdomainEntry)],
        tailnet: &[(&str, &str)],
    ) -> DiscoveredSubdomains {
        DiscoveredSubdomains {
            public: public
                .iter()
                .map(|(app, e)| {
                    (
                        (*app).to_string(),
                        SubdomainEntry {
                            subdomain: e.subdomain.clone(),
                            ip_override: e.ip_override.clone(),
                        },
                    )
                })
                .collect(),
            tailnet_only: tailnet
                .iter()
                .map(|(app, sub)| ((*app).to_string(), entry(sub)))
                .collect(),
        }
    }

    fn plan(
        discovered: DiscoveredSubdomains,
        subdomains: &[&str],
        skip: &[&str],
    ) -> Result<SetAllPlan> {
        let skip_set: HashSet<String> = skip.iter().map(|s| (*s).to_string()).collect();
        plan_set_all(
            "example.com",
            "203.0.113.10",
            discovered,
            subdomains.iter().map(|s| (*s).to_string()).collect(),
            &skip_set,
        )
    }

    #[test]
    fn plan_implicit_partitions_public_and_tailnet_only() {
        let p = plan(
            discovered(&[("freshrss", entry("rss"))], &[("bichon", "bichon")]),
            &[],
            &[],
        )
        .unwrap();

        assert_eq!(p.to_create.len(), 1);
        assert_eq!(p.to_create[0].app, "freshrss");
        assert_eq!(p.to_create[0].subdomain, "rss");
        assert_eq!(p.to_create[0].fqdn, "rss.example.com");
        assert_eq!(p.to_create[0].ip, "203.0.113.10");
        assert_eq!(
            p.skipped,
            vec![SkippedApp {
                app: "bichon".to_string(),
                subdomain: "bichon".to_string(),
                reason: SkipReason::TailnetOnly,
            }]
        );
    }

    #[test]
    fn plan_implicit_skip_excludes_tailnet_only_from_skipped_list() {
        let p = plan(
            discovered(&[], &[("bichon", "bichon"), ("paperless", "paperless")]),
            &[],
            &["bichon"],
        )
        .unwrap();
        let names: Vec<&str> = p.skipped.iter().map(|s| s.app.as_str()).collect();
        assert_eq!(names, vec!["paperless"]);
    }

    #[test]
    fn plan_implicit_lists_are_sorted_alphabetically() {
        let p = plan(
            discovered(
                &[
                    ("navidrome", entry("music")),
                    ("baikal", entry("baikal")),
                    ("freshrss", entry("rss")),
                ],
                &[
                    ("paperless", "paperless"),
                    ("bichon", "bichon"),
                    ("cockpit", "cockpit"),
                ],
            ),
            &[],
            &[],
        )
        .unwrap();

        let created: Vec<&str> = p.to_create.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(created, vec!["baikal", "freshrss", "navidrome"]);
        let skipped: Vec<&str> = p.skipped.iter().map(|s| s.app.as_str()).collect();
        assert_eq!(skipped, vec!["bichon", "cockpit", "paperless"]);
    }

    #[test]
    fn plan_implicit_skip_excludes_public_app_from_creation() {
        let p = plan(
            discovered(
                &[("freshrss", entry("rss")), ("baikal", entry("baikal"))],
                &[],
            ),
            &[],
            &["freshrss"],
        )
        .unwrap();
        let created: Vec<&str> = p.to_create.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(created, vec!["baikal"]);
    }

    #[test]
    fn plan_explicit_tailnet_only_target_errors_before_returning() {
        let err = plan(
            discovered(&[], &[("paperless", "docs")]),
            &["paperless"],
            &[],
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("paperless"), "error must name the offender");
        assert!(
            msg.contains("subdomain: docs"),
            "error must surface effective subdomain"
        );
        assert!(msg.contains("ADR-0003"), "error must reference the ADR");
        assert!(
            msg.contains("auberge deploy"),
            "error must point to the corrective action"
        );
    }

    #[test]
    fn plan_explicit_skip_excludes_tailnet_only_target_avoids_error() {
        let p = plan(
            discovered(&[], &[("bichon", "bichon")]),
            &["bichon"],
            &["bichon"],
        )
        .unwrap();
        assert!(p.to_create.is_empty());
        assert!(
            p.skipped.is_empty(),
            "explicit branch returns empty skip list — operator owns the choice"
        );
    }

    #[test]
    fn plan_explicit_public_only_returns_empty_skip() {
        let p = plan(
            discovered(
                &[("freshrss", entry("rss")), ("baikal", entry("baikal"))],
                &[],
            ),
            &["freshrss"],
            &[],
        )
        .unwrap();
        let created: Vec<&str> = p.to_create.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(created, vec!["freshrss"]);
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn plan_explicit_unknown_app_silently_dropped() {
        let p = plan(discovered(&[], &[]), &["nonexistent"], &[]).unwrap();
        assert!(p.to_create.is_empty());
        assert!(p.skipped.is_empty());
    }

    // A per-app `_tailscale_ip` beats the run's target IP: the App answers on
    // the tailnet even though it is published as a public A record.
    #[test]
    fn plan_ip_override_beats_the_target_ip() {
        let p = plan(
            discovered(
                &[
                    (
                        "grimmory",
                        SubdomainEntry {
                            subdomain: "grimmory".to_string(),
                            ip_override: Some("100.101.255.46".to_string()),
                        },
                    ),
                    ("freshrss", entry("rss")),
                ],
                &[],
            ),
            &[],
            &[],
        )
        .unwrap();

        let ips: Vec<(&str, &str)> = p
            .to_create
            .iter()
            .map(|r| (r.app.as_str(), r.ip.as_str()))
            .collect();
        assert_eq!(
            ips,
            vec![("freshrss", "203.0.113.10"), ("grimmory", "100.101.255.46")]
        );
    }

    // ── apply_set_all ─────────────────────────────────────────────────────────

    fn apply_plan(records: &[(&str, &str)]) -> SetAllPlan {
        SetAllPlan {
            to_create: records
                .iter()
                .map(|(app, sub)| PlannedRecord {
                    app: (*app).to_string(),
                    subdomain: (*sub).to_string(),
                    fqdn: format!("{sub}.example.com"),
                    ip: "203.0.113.10".to_string(),
                })
                .collect(),
            skipped: vec![],
        }
    }

    #[tokio::test]
    async fn apply_writes_every_planned_record() {
        let dns = FakeDns::new("example.com", vec![]);
        let plan = apply_plan(&[("baikal", "baikal"), ("freshrss", "rss")]);
        let outcome = apply_set_all(&dns, &plan, false, Duration::ZERO, &mut |_| {}).await;

        assert_eq!(
            dns.writes(),
            vec![
                ("baikal".to_string(), "203.0.113.10".to_string()),
                ("rss".to_string(), "203.0.113.10".to_string()),
            ]
        );
        assert_eq!(outcome.created.len(), 2);
        assert!(outcome.failed.is_empty());
        assert!(outcome.created.iter().all(|r| r.error.is_none()));
    }

    // The failure path the command reads its exit status off. A write that
    // errors lands in `failed` carrying the provider's message — it is not
    // propagated, so both output modes still get a summary.
    #[tokio::test]
    async fn apply_records_a_failure_instead_of_propagating_it() {
        let dns = FakeDns::new("example.com", vec![]).failing_on(&["rss"]);
        let plan = apply_plan(&[("baikal", "baikal"), ("freshrss", "rss")]);
        let outcome = apply_set_all(&dns, &plan, true, Duration::ZERO, &mut |_| {}).await;

        assert_eq!(outcome.created.len(), 1);
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(outcome.failed[0].fqdn, "rss.example.com");
        assert_eq!(
            outcome.failed[0].error.as_deref(),
            Some("cloudflare rejected rss")
        );
    }

    #[tokio::test]
    async fn apply_continue_on_error_keeps_going_past_a_failure() {
        let dns = FakeDns::new("example.com", vec![]).failing_on(&["baikal"]);
        let plan = apply_plan(&[("baikal", "baikal"), ("freshrss", "rss")]);
        let outcome = apply_set_all(&dns, &plan, true, Duration::ZERO, &mut |_| {}).await;

        assert_eq!(dns.writes().len(), 2, "second record must still be tried");
        assert_eq!(outcome.created.len(), 1);
        assert_eq!(outcome.failed.len(), 1);
    }

    #[tokio::test]
    async fn apply_fail_fast_stops_at_the_first_failure() {
        let dns = FakeDns::new("example.com", vec![]).failing_on(&["baikal"]);
        let plan = apply_plan(&[("baikal", "baikal"), ("freshrss", "rss")]);
        let outcome = apply_set_all(&dns, &plan, false, Duration::ZERO, &mut |_| {}).await;

        assert_eq!(
            dns.writes(),
            vec![("baikal".to_string(), "203.0.113.10".to_string())],
            "no record after the failure may be written"
        );
        assert!(outcome.created.is_empty());
        assert_eq!(outcome.failed.len(), 1);
    }

    // Reporting only what it attempted would drop the abandoned records from a
    // plan the reader cannot see: with three planned and the first failing, the
    // body would name one subdomain and silently lose two.
    #[tokio::test]
    async fn apply_fail_fast_returns_the_records_it_abandoned() {
        let dns = FakeDns::new("example.com", vec![]).failing_on(&["baikal"]);
        let plan = apply_plan(&[
            ("baikal", "baikal"),
            ("freshrss", "rss"),
            ("navidrome", "music"),
        ]);
        let outcome = apply_set_all(&dns, &plan, false, Duration::ZERO, &mut |_| {}).await;

        let abandoned: Vec<&str> = outcome
            .not_attempted
            .iter()
            .map(|r| r.app.as_str())
            .collect();
        assert_eq!(abandoned, vec!["freshrss", "navidrome"]);
    }

    /// `created`, `failed` and `not_attempted` partition the plan on every
    /// path, so a reader can always reconcile the report against what was
    /// planned.
    async fn assert_outcome_partitions_the_plan(fail_on: &[&str], continue_on_error: bool) {
        let dns = FakeDns::new("example.com", vec![]).failing_on(fail_on);
        let plan = apply_plan(&[
            ("baikal", "baikal"),
            ("freshrss", "rss"),
            ("navidrome", "music"),
        ]);
        let outcome =
            apply_set_all(&dns, &plan, continue_on_error, Duration::ZERO, &mut |_| {}).await;

        assert_eq!(
            outcome.created.len() + outcome.failed.len() + outcome.not_attempted.len(),
            plan.to_create.len(),
            "fail_on={fail_on:?} continue_on_error={continue_on_error} left records unaccounted for"
        );
    }

    #[tokio::test]
    async fn apply_accounts_for_every_planned_record_on_every_path() {
        assert_outcome_partitions_the_plan(&[], false).await;
        assert_outcome_partitions_the_plan(&["baikal"], false).await;
        assert_outcome_partitions_the_plan(&["rss"], false).await;
        assert_outcome_partitions_the_plan(&["music"], false).await;
        assert_outcome_partitions_the_plan(&["baikal", "music"], true).await;
        assert_outcome_partitions_the_plan(&["baikal", "rss", "music"], true).await;
    }

    // continue_on_error tries everything, so nothing is abandoned.
    #[tokio::test]
    async fn apply_continue_on_error_abandons_nothing() {
        let dns = FakeDns::new("example.com", vec![]).failing_on(&["baikal"]);
        let plan = apply_plan(&[("baikal", "baikal"), ("freshrss", "rss")]);
        let outcome = apply_set_all(&dns, &plan, true, Duration::ZERO, &mut |_| {}).await;

        assert!(outcome.not_attempted.is_empty());
    }

    // Human output streams each write as it lands rather than after the whole
    // paced run, so the callback fires once per attempt, in plan order.
    #[tokio::test]
    async fn apply_reports_each_write_as_it_lands() {
        let dns = FakeDns::new("example.com", vec![]).failing_on(&["rss"]);
        let plan = apply_plan(&[("baikal", "baikal"), ("freshrss", "rss")]);
        let mut seen: Vec<(String, bool)> = vec![];
        apply_set_all(&dns, &plan, true, Duration::ZERO, &mut |applied| {
            seen.push((applied.fqdn.clone(), applied.error.is_none()));
        })
        .await;

        assert_eq!(
            seen,
            vec![
                ("baikal.example.com".to_string(), true),
                ("rss.example.com".to_string(), false),
            ]
        );
    }

    // ── migrate_all ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn migrate_repoints_public_a_records() {
        let dns = FakeDns::new(
            "example.com",
            vec![
                a_record("rss.example.com", "203.0.113.10"),
                a_record("baikal.example.com", "203.0.113.10"),
            ],
        );
        let outcome = migrate_all(&dns, "198.51.100.7", false).await.unwrap();

        let mut moved: Vec<&str> = outcome
            .migrated
            .iter()
            .map(|r| r.subdomain.as_str())
            .collect();
        moved.sort();
        assert_eq!(moved, vec!["baikal", "rss"]);
        assert!(
            outcome
                .migrated
                .iter()
                .all(|r| r.success && r.new_ip == "198.51.100.7")
        );
        assert!(outcome.skipped.is_empty());
        assert_eq!(dns.writes().len(), 2);
    }

    // A record already holding a CGNAT address is a Tailnet-only App's
    // publication, not a Host address — repointing it would take the App off
    // the tailnet (ADR-0003). The skip is an answer, not a footnote: a caller
    // diffing the migration against the zone reads it off `skipped` rather
    // than off stderr.
    #[tokio::test]
    async fn migrate_leaves_tailscale_records_alone_and_reports_them_as_data() {
        let dns = FakeDns::new(
            "example.com",
            vec![
                a_record("rss.example.com", "203.0.113.10"),
                a_record("grimmory.example.com", "100.101.255.46"),
                a_record("bichon.example.com", "100.64.0.9"),
            ],
        );
        let outcome = migrate_all(&dns, "198.51.100.7", false).await.unwrap();

        let moved: Vec<&str> = outcome
            .migrated
            .iter()
            .map(|r| r.subdomain.as_str())
            .collect();
        assert_eq!(moved, vec!["rss"]);
        assert_eq!(
            dns.writes(),
            vec![("rss".to_string(), "198.51.100.7".to_string())]
        );
        assert_eq!(
            outcome.skipped,
            vec![
                SkippedRecord {
                    subdomain: "grimmory".to_string(),
                    ip: "100.101.255.46".to_string(),
                    reason: SkipReason::TailnetOnly,
                },
                SkippedRecord {
                    subdomain: "bichon".to_string(),
                    ip: "100.64.0.9".to_string(),
                    reason: SkipReason::TailnetOnly,
                },
            ],
            "one row per skipped record, in zone order, with the reason named"
        );
    }

    // Nothing to migrate is not nothing to report.
    #[tokio::test]
    async fn migrate_over_a_tailnet_only_zone_writes_nothing_and_still_reports() {
        let dns = FakeDns::new(
            "example.com",
            vec![a_record("bichon.example.com", "100.100.200.1")],
        );
        let outcome = migrate_all(&dns, "198.51.100.7", false).await.unwrap();

        assert!(outcome.migrated.is_empty());
        assert!(dns.writes().is_empty());
        assert_eq!(outcome.skipped.len(), 1);
    }

    // The apex, other domains and non-A records were never candidates, so
    // naming them would inflate the array a caller diffs against the zone.
    #[tokio::test]
    async fn migrate_ignores_non_a_records_the_apex_and_other_domains() {
        let dns = FakeDns::new(
            "example.com",
            vec![
                a_record("example.com", "203.0.113.10"),
                a_record("rss.other.test", "203.0.113.10"),
                DnsRecord {
                    id: "cname".to_string(),
                    name: "www.example.com".to_string(),
                    ttl: 1,
                    content: RecordContent::Cname {
                        target: "example.com".to_string(),
                    },
                },
            ],
        );
        let outcome = migrate_all(&dns, "198.51.100.7", false).await.unwrap();

        assert!(outcome.migrated.is_empty());
        assert!(
            outcome.skipped.is_empty(),
            "out-of-scope records are not ADR-0003 skips"
        );
        assert!(dns.writes().is_empty());
    }

    // A dry run decides the same skips as a real one — the preview is only
    // trustworthy if it reports them.
    #[tokio::test]
    async fn migrate_dry_run_reports_without_writing() {
        let dns = FakeDns::new(
            "example.com",
            vec![
                a_record("rss.example.com", "203.0.113.10"),
                a_record("bichon.example.com", "100.64.0.9"),
            ],
        );
        let outcome = migrate_all(&dns, "198.51.100.7", true).await.unwrap();

        assert_eq!(outcome.migrated.len(), 1);
        assert_eq!(outcome.migrated[0].old_ip, "203.0.113.10");
        assert!(outcome.migrated[0].success);
        assert_eq!(
            outcome.skipped,
            vec![SkippedRecord {
                subdomain: "bichon".to_string(),
                ip: "100.64.0.9".to_string(),
                reason: SkipReason::TailnetOnly,
            }]
        );
        assert!(dns.writes().is_empty(), "a dry run must write nothing");
    }

    // ── status ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn status_reports_configured_subdomains_with_no_a_record() {
        let dns = FakeDns::new(
            "example.com",
            vec![
                a_record("rss.example.com", "203.0.113.10"),
                DnsRecord {
                    id: "cname".to_string(),
                    name: "baikal.example.com".to_string(),
                    ttl: 1,
                    content: RecordContent::Cname {
                        target: "example.com".to_string(),
                    },
                },
            ],
        );
        let report = status(
            &dns,
            vec!["rss".to_string(), "baikal".to_string(), "music".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(report.domain, "example.com");
        assert_eq!(report.active_records.len(), 2);
        assert_eq!(
            report.missing_subdomains,
            vec!["baikal".to_string(), "music".to_string()],
            "a CNAME does not publish the App — only an A record counts"
        );
    }
}
