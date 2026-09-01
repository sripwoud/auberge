use crate::config::Config;
use crate::hosts::{Host, serving_hosts};
use crate::services::dns::is_tailscale_ip;
use eyre::Result;
use std::net::IpAddr;

/// A failed DNS A-record verification.
#[derive(Debug, PartialEq)]
pub enum VerifyFailure {
    /// The A record exists but doesn't include the expected IP.
    Mismatch {
        /// The actual A records returned.
        got: Vec<String>,
    },
    /// No A records found (NXDOMAIN or empty answer).
    NxDomain,
}

/// Trait for DNS A-record lookup, enabling test doubles.
pub trait DnsLookup {
    fn lookup_ipv4(&self, fqdn: &str) -> Result<Vec<IpAddr>>;
}

/// Production DNS lookup using hickory-resolver (queries a specific resolver IP at UDP/53).
pub struct HickoryLookup {
    resolver: hickory_resolver::TokioResolver,
}

impl HickoryLookup {
    pub fn new(resolver_ip: &str) -> Result<Self> {
        use hickory_resolver::{
            TokioResolver,
            config::{NameServerConfig, ResolverConfig, ResolverOpts},
            net::runtime::TokioRuntimeProvider,
        };

        let addr: IpAddr = resolver_ip
            .parse()
            .map_err(|e| eyre::eyre!("Invalid resolver IP '{resolver_ip}': {e}"))?;

        let ns = NameServerConfig::udp(addr);
        let config = ResolverConfig::from_parts(None, vec![], vec![ns]);
        let mut opts = ResolverOpts::default();
        opts.attempts = 2;

        let resolver = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
            .with_options(opts)
            .build()
            .map_err(|e| eyre::eyre!("Failed to build DNS resolver: {e}"))?;

        Ok(Self { resolver })
    }
}

impl DnsLookup for HickoryLookup {
    fn lookup_ipv4(&self, fqdn: &str) -> Result<Vec<IpAddr>> {
        let fqdn_owned;
        let fqdn_dot: &str = if fqdn.ends_with('.') {
            fqdn
        } else {
            fqdn_owned = format!("{fqdn}.");
            &fqdn_owned
        };

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match self.resolver.lookup_ip(fqdn_dot).await {
                    Ok(lookup) => Ok(lookup.iter().filter(|a: &IpAddr| a.is_ipv4()).collect()),
                    Err(e) if e.is_no_records_found() => Ok(vec![]),
                    Err(e) => Err(eyre::eyre!("DNS lookup error: {e}")),
                }
            })
        })
    }
}

/// Compare the DNS lookup result against `expected_ip`. Returns `Ok(None)` on
/// match, `Ok(Some(failure))` for mismatch / NXDOMAIN, and `Err` for I/O errors
/// or when `expected_ip` is not a valid IP literal.
pub fn verify_a_record<L: DnsLookup>(
    lookup: &L,
    fqdn: &str,
    expected_ip: &str,
) -> Result<Option<VerifyFailure>> {
    let expected: IpAddr = expected_ip
        .parse()
        .map_err(|e| eyre::eyre!("Invalid expected IP '{expected_ip}': {e}"))?;
    let ips = lookup.lookup_ipv4(fqdn)?;
    if ips.is_empty() {
        return Ok(Some(VerifyFailure::NxDomain));
    }
    if ips.contains(&expected) {
        Ok(None)
    } else {
        Ok(Some(VerifyFailure::Mismatch {
            got: ips.iter().map(|ip| ip.to_string()).collect(),
        }))
    }
}

/// Resolved verification parameters for a single app.
#[derive(Debug)]
pub struct AppVerifyConfig {
    pub fqdn: String,
    pub resolver_ip: String,
    pub expected_ip: String,
}

impl AppVerifyConfig {
    /// `true` when the check targets Blocky over the tailnet
    /// (resolver IP is in the Tailscale CGNAT range).
    pub fn is_tailnet(&self) -> bool {
        is_tailscale_ip(&self.resolver_ip)
    }
}

/// The config key that gates Blocky onto a Host (ADR-0051, ADR-0058).
const BLOCKY_GATE: &str = "blocky_subdomain";

/// Where the tailnet's resolver answers, or why the CLI cannot say.
///
/// A Tailnet-only App's records are published in the Blocky the tailnet
/// resolves through (ADR-0052), which stops being the App's own Host as soon
/// as the fleet is larger than one — so ADR-0003's fifth decision bullet says
/// the check queries Blocky, and the CLI has to find it.
#[derive(Debug)]
pub enum TailnetResolver {
    At(String),
    Unlocatable(String),
}

impl TailnetResolver {
    /// Which Host runs Blocky, and what is its tailnet address? The Host is
    /// the one whose config answers the Blocky gate; the address is the one
    /// `auberge host detect-tailscale-ip` cached for it in `hosts.toml`.
    ///
    /// Every way of not knowing is a reason, never a silent fallback: an
    /// address guessed here would be verified against, and a check that passes
    /// against the wrong resolver says nothing at all.
    pub fn locate(hosts: &[Host], config: &Config) -> Self {
        if hosts.is_empty() {
            return Self::Unlocatable(
                "hosts.toml lists no Host, so there is nothing to run the tailnet's resolver"
                    .to_string(),
            );
        }

        let serving = serving_hosts(hosts, config, BLOCKY_GATE);
        let host = match serving.as_slice() {
            [only] => only,
            [] => {
                return Self::Unlocatable(format!(
                    "no Host's config answers `{BLOCKY_GATE}`, so the tailnet has no resolver to query"
                ));
            }
            several => {
                let names: Vec<&str> = several.iter().map(|h| h.name.as_str()).collect();
                return Self::Unlocatable(format!(
                    "{} Hosts answer `{BLOCKY_GATE}` ({}), but the tailnet has one resolver (ADR-0052); \
                     withdraw the gate on the others with `[hosts.<name>] {BLOCKY_GATE} = \"\"`",
                    names.len(),
                    names.join(", ")
                ));
            }
        };

        match host
            .tailscale_ip
            .as_deref()
            .map(str::trim)
            .filter(|ip| !ip.is_empty())
        {
            Some(ip) if is_tailscale_ip(ip) => Self::At(ip.to_string()),
            Some(ip) => Self::Unlocatable(format!(
                "Host '{}' serves Blocky but its recorded tailscale_ip '{ip}' is not a Tailscale address",
                host.name
            )),
            None => Self::Unlocatable(format!(
                "Host '{name}' serves Blocky but hosts.toml records no tailscale_ip for it; \
                 run `auberge host detect-tailscale-ip {name}`",
                name = host.name
            )),
        }
    }
}

/// Derive the DNS-verification config for `app` from the user config.
///
/// A Tailnet-only App is two addresses, not one: `tailnet_resolver` answers
/// the query and `{app}_tailscale_ip` is the answer it must give. They coincide
/// only while the App runs on the resolver's own Host.
///
/// Returns `Ok(None)` when:
/// - the app has no `{app}_subdomain` config key, or
/// - the app is public and `verify_public` is `false`.
///
/// Returns `Err` when the app is Tailnet-only and the resolver is unlocatable:
/// its records live in that resolver or nowhere, so an unanswerable check is a
/// failed one.
pub fn app_verify_config(
    app: &str,
    domain: &str,
    public_address: &str,
    config: &Config,
    host: Option<&str>,
    verify_public: bool,
    tailnet_resolver: &TailnetResolver,
) -> Result<Option<AppVerifyConfig>> {
    let subdomain_key = format!("{}_subdomain", app);
    let Some(subdomain) = config
        .get_for_host(&subdomain_key, host)
        .filter(|v| !v.is_empty())
    else {
        return Ok(None);
    };
    let fqdn = format!("{}.{}", subdomain, domain);

    let tailscale_key = format!("{}_tailscale_ip", app);
    if let Some(tailscale_ip) = config
        .get_for_host(&tailscale_key, host)
        .filter(|v| !v.is_empty())
        && is_tailscale_ip(&tailscale_ip)
    {
        let resolver_ip = match tailnet_resolver {
            TailnetResolver::At(ip) => ip.clone(),
            TailnetResolver::Unlocatable(why) => {
                eyre::bail!("Cannot verify {fqdn} on the tailnet: {why}")
            }
        };
        return Ok(Some(AppVerifyConfig {
            fqdn,
            resolver_ip,
            expected_ip: tailscale_ip,
        }));
    }

    if verify_public {
        return Ok(Some(AppVerifyConfig {
            fqdn,
            resolver_ip: "1.1.1.1".to_string(),
            expected_ip: public_address.to_string(),
        }));
    }

    Ok(None)
}

/// Format a user-visible diagnostic for a failed DNS check.
pub fn format_dns_error(
    fqdn: &str,
    resolver_ip: &str,
    expected_ip: &str,
    failure: &VerifyFailure,
) -> String {
    match failure {
        VerifyFailure::Mismatch { got } => format!(
            "DNS mismatch for {fqdn}: queried {resolver_ip}, expected {expected_ip}, got [{}]",
            got.join(", ")
        ),
        VerifyFailure::NxDomain => format!(
            "DNS check failed for {fqdn}: queried {resolver_ip}, expected {expected_ip}, got NXDOMAIN (name not found)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};

    // ── Mock resolver ─────────────────────────────────────────────────────────

    enum MockResult {
        Found(Vec<IpAddr>),
        Empty,
        Error(String),
    }

    struct MockLookup {
        results: HashMap<String, MockResult>,
    }

    impl MockLookup {
        fn new() -> Self {
            Self {
                results: HashMap::new(),
            }
        }

        fn with_found(mut self, fqdn: &str, ips: Vec<Ipv4Addr>) -> Self {
            self.results.insert(
                fqdn.to_string(),
                MockResult::Found(ips.into_iter().map(IpAddr::V4).collect()),
            );
            self
        }

        fn with_nxdomain(mut self, fqdn: &str) -> Self {
            self.results.insert(fqdn.to_string(), MockResult::Empty);
            self
        }

        fn with_error(mut self, fqdn: &str, msg: &str) -> Self {
            self.results
                .insert(fqdn.to_string(), MockResult::Error(msg.to_string()));
            self
        }
    }

    impl DnsLookup for MockLookup {
        fn lookup_ipv4(&self, fqdn: &str) -> Result<Vec<IpAddr>> {
            match self.results.get(fqdn) {
                Some(MockResult::Found(ips)) => Ok(ips.clone()),
                Some(MockResult::Empty) => Ok(vec![]),
                Some(MockResult::Error(msg)) => Err(eyre::eyre!("{}", msg)),
                None => Ok(vec![]),
            }
        }
    }

    // ── verify_a_record ───────────────────────────────────────────────────────

    #[test]
    fn test_verify_match_tailnet() {
        let ip = "100.64.1.2";
        let fqdn = "myapp.example.ts";
        let lookup = MockLookup::new().with_found(fqdn, vec![ip.parse::<Ipv4Addr>().unwrap()]);
        assert_eq!(verify_a_record(&lookup, fqdn, ip).unwrap(), None);
    }

    #[test]
    fn test_verify_match_public() {
        let ip = "203.0.113.10";
        let fqdn = "app.example.com";
        let lookup = MockLookup::new().with_found(fqdn, vec![ip.parse::<Ipv4Addr>().unwrap()]);
        assert_eq!(verify_a_record(&lookup, fqdn, ip).unwrap(), None);
    }

    #[test]
    fn test_verify_mismatch() {
        let fqdn = "app.example.com";
        let actual_ip = "203.0.113.99";
        let lookup =
            MockLookup::new().with_found(fqdn, vec![actual_ip.parse::<Ipv4Addr>().unwrap()]);
        let failure = verify_a_record(&lookup, fqdn, "203.0.113.10").unwrap();
        assert_eq!(
            failure,
            Some(VerifyFailure::Mismatch {
                got: vec![actual_ip.to_string()]
            })
        );
    }

    #[test]
    fn test_verify_nxdomain() {
        let fqdn = "missing.example.com";
        let lookup = MockLookup::new().with_nxdomain(fqdn);
        assert_eq!(
            verify_a_record(&lookup, fqdn, "203.0.113.10").unwrap(),
            Some(VerifyFailure::NxDomain)
        );
    }

    #[test]
    fn test_verify_lookup_error_propagated() {
        let fqdn = "app.example.com";
        let lookup = MockLookup::new().with_error(fqdn, "timeout");
        assert!(verify_a_record(&lookup, fqdn, "203.0.113.10").is_err());
    }

    #[test]
    fn test_verify_invalid_expected_ip_errors() {
        let fqdn = "app.example.com";
        let lookup = MockLookup::new().with_found(fqdn, vec!["1.2.3.4".parse().unwrap()]);
        let err = verify_a_record(&lookup, fqdn, "not-an-ip").unwrap_err();
        assert!(err.to_string().contains("Invalid expected IP"));
    }

    // ── TailnetResolver::locate ───────────────────────────────────────────────

    fn unlocatable(resolver: &TailnetResolver) -> &str {
        match resolver {
            TailnetResolver::At(ip) => panic!("expected Unlocatable, got At({ip})"),
            TailnetResolver::Unlocatable(why) => why,
        }
    }

    #[test]
    fn test_locate_reads_the_gated_hosts_cached_address() {
        let hosts = [
            Host::fixture("auberge", Some("100.64.0.1")),
            Host::fixture("ruche", Some("100.64.0.9")),
        ];
        let config = make_config(
            r#"
blocky_subdomain = "dns"

[hosts.ruche]
blocky_subdomain = ""
"#,
        );
        match TailnetResolver::locate(&hosts, &config) {
            TailnetResolver::At(ip) => assert_eq!(ip, "100.64.0.1"),
            TailnetResolver::Unlocatable(why) => panic!("expected At, got Unlocatable({why})"),
        }
    }

    #[test]
    fn test_locate_unlocatable_when_the_roster_is_empty() {
        let config = make_config(r#"blocky_subdomain = "dns""#);
        let resolver = TailnetResolver::locate(&[], &config);
        assert!(unlocatable(&resolver).contains("hosts.toml"));
    }

    #[test]
    fn test_locate_unlocatable_when_no_host_answers_the_gate() {
        let hosts = [Host::fixture("auberge", Some("100.64.0.1"))];
        let config = make_config(r#"domain = "example.com""#);
        let why = TailnetResolver::locate(&hosts, &config);
        assert!(unlocatable(&why).contains("blocky_subdomain"), "{why:?}");
    }

    #[test]
    fn test_locate_unlocatable_when_several_hosts_answer_the_gate() {
        let hosts = [
            Host::fixture("auberge", Some("100.64.0.1")),
            Host::fixture("ruche", Some("100.64.0.9")),
        ];
        let config = make_config(r#"blocky_subdomain = "dns""#);
        let resolver = TailnetResolver::locate(&hosts, &config);
        let why = unlocatable(&resolver);
        assert!(why.contains("auberge"), "{why}");
        assert!(why.contains("ruche"), "{why}");
    }

    #[test]
    fn test_locate_unlocatable_when_the_gated_host_has_no_cached_address() {
        let hosts = [Host::fixture("auberge", None)];
        let config = make_config(r#"blocky_subdomain = "dns""#);
        let resolver = TailnetResolver::locate(&hosts, &config);
        let why = unlocatable(&resolver);
        assert!(why.contains("detect-tailscale-ip"), "{why}");
        assert!(why.contains("auberge"), "{why}");
    }

    #[test]
    fn test_locate_unlocatable_when_the_cached_address_is_not_a_tailnet_one() {
        let hosts = [Host::fixture("auberge", Some("192.168.1.10"))];
        let config = make_config(r#"blocky_subdomain = "dns""#);
        let resolver = TailnetResolver::locate(&hosts, &config);
        assert!(unlocatable(&resolver).contains("192.168.1.10"));
    }

    // ── app_verify_config ─────────────────────────────────────────────────────

    fn make_config(toml_str: &str) -> Config {
        Config::from_toml_str(toml_str).expect("test fixture TOML must parse")
    }

    fn resolver_at(ip: &str) -> TailnetResolver {
        TailnetResolver::At(ip.to_string())
    }

    #[test]
    fn test_app_verify_config_tailnet_queries_the_resolver_for_the_apps_address() {
        let config = make_config(
            r#"
domain = "example.com"
aoe_subdomain = "essaim"
aoe_tailscale_ip = "100.64.0.9"
"#,
        );
        let vc = app_verify_config(
            "aoe",
            "example.com",
            "1.2.3.4",
            &config,
            None,
            false,
            &resolver_at("100.64.0.1"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(vc.fqdn, "essaim.example.com");
        assert_eq!(vc.resolver_ip, "100.64.0.1");
        assert_eq!(vc.expected_ip, "100.64.0.9");
        assert!(vc.is_tailnet());
    }

    #[test]
    fn test_app_verify_config_tailnet_on_the_resolvers_own_host() {
        let config = make_config(
            r#"
domain = "example.com"
paperless_subdomain = "paperless"
paperless_tailscale_ip = "100.64.0.1"
"#,
        );
        let vc = app_verify_config(
            "paperless",
            "example.com",
            "1.2.3.4",
            &config,
            None,
            false,
            &resolver_at("100.64.0.1"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(vc.resolver_ip, "100.64.0.1");
        assert_eq!(vc.expected_ip, "100.64.0.1");
    }

    #[test]
    fn test_app_verify_config_tailnet_errors_when_the_resolver_is_unlocatable() {
        let config = make_config(
            r#"
domain = "example.com"
aoe_subdomain = "essaim"
aoe_tailscale_ip = "100.64.0.9"
"#,
        );
        let err = app_verify_config(
            "aoe",
            "example.com",
            "1.2.3.4",
            &config,
            None,
            false,
            &TailnetResolver::Unlocatable("no Host answers the gate".to_string()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("essaim.example.com"), "{err}");
        assert!(
            err.to_string().contains("no Host answers the gate"),
            "{err}"
        );
    }

    #[test]
    fn test_app_verify_config_public_ignores_an_unlocatable_resolver() {
        let config = make_config(
            r#"
domain = "example.com"
freshrss_subdomain = "rss"
"#,
        );
        let vc = app_verify_config(
            "freshrss",
            "example.com",
            "203.0.113.10",
            &config,
            None,
            true,
            &TailnetResolver::Unlocatable("no Host answers the gate".to_string()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(vc.fqdn, "rss.example.com");
        assert_eq!(vc.resolver_ip, "1.1.1.1");
        assert_eq!(vc.expected_ip, "203.0.113.10");
        assert!(!vc.is_tailnet());
    }

    #[test]
    fn test_app_verify_config_public_opt_out() {
        let config = make_config(
            r#"
domain = "example.com"
freshrss_subdomain = "rss"
"#,
        );
        assert!(
            app_verify_config(
                "freshrss",
                "example.com",
                "203.0.113.10",
                &config,
                None,
                false,
                &resolver_at("100.64.0.1"),
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn test_app_verify_config_no_subdomain() {
        let config = make_config(r#"domain = "example.com""#);
        assert!(
            app_verify_config(
                "paperless",
                "example.com",
                "1.2.3.4",
                &config,
                None,
                true,
                &resolver_at("100.64.0.1"),
            )
            .unwrap()
            .is_none()
        );
    }

    // ── format_dns_error ──────────────────────────────────────────────────────

    #[test]
    fn test_format_dns_error_mismatch() {
        let msg = format_dns_error(
            "app.example.com",
            "100.64.1.2",
            "100.64.1.2",
            &VerifyFailure::Mismatch {
                got: vec!["203.0.113.99".to_string()],
            },
        );
        assert!(msg.contains("app.example.com"));
        assert!(msg.contains("100.64.1.2"));
        assert!(msg.contains("203.0.113.99"));
    }

    #[test]
    fn test_format_dns_error_nxdomain() {
        let msg = format_dns_error(
            "app.example.com",
            "1.1.1.1",
            "203.0.113.10",
            &VerifyFailure::NxDomain,
        );
        assert!(msg.contains("app.example.com"));
        assert!(msg.contains("1.1.1.1"));
        assert!(msg.contains("NXDOMAIN"));
    }

    // ── HickoryLookup network integration ─────────────────────────────────────
    //
    // Exercises the real resolver wiring (TokioResolver build, block_in_place,
    // trailing-dot FQDN, IPv4 filter). Ignored by default because it hits the
    // public network. Run with:
    //
    //     cargo nextest run --run-ignored only -- hickory_lookup
    //
    // Cloudflare publishes one.one.one.one → 1.1.1.1 / 1.0.0.1 as a stable
    // anchor; we just check that querying 1.1.1.1 returns that IP.

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires network access to 1.1.1.1"]
    async fn hickory_lookup_resolves_public_anchor() {
        let lookup = HickoryLookup::new("1.1.1.1").expect("build resolver");
        let ips = lookup
            .lookup_ipv4("one.one.one.one")
            .expect("lookup succeeds");
        let one_one: IpAddr = "1.1.1.1".parse().unwrap();
        assert!(
            ips.contains(&one_one),
            "expected 1.1.1.1 in {ips:?} for one.one.one.one"
        );
    }
}
