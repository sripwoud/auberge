use crate::config::Config;
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
pub struct AppVerifyConfig {
    pub fqdn: String,
    pub resolver_ip: String,
    pub expected_ip: String,
    /// `true` when the check targets Blocky over the tailnet.
    pub is_tailnet: bool,
}

/// Derive the DNS-verification config for `app` from the user config.
///
/// Returns `None` when:
/// - the app has no `{app}_subdomain` config key, or
/// - the app is public and `verify_public` is `false`.
pub fn app_verify_config(
    app: &str,
    domain: &str,
    ansible_host: &str,
    config: &Config,
    verify_public: bool,
) -> Option<AppVerifyConfig> {
    let subdomain_key = format!("{}_subdomain", app);
    let subdomain = config.get(&subdomain_key).filter(|v| !v.is_empty())?;
    let fqdn = format!("{}.{}", subdomain, domain);

    let tailscale_key = format!("{}_tailscale_ip", app);
    if let Some(tailscale_ip) = config.get(&tailscale_key).filter(|v| !v.is_empty())
        && is_tailscale_ip(&tailscale_ip)
    {
        return Some(AppVerifyConfig {
            fqdn,
            resolver_ip: tailscale_ip.clone(),
            expected_ip: tailscale_ip,
            is_tailnet: true,
        });
    }

    if verify_public {
        return Some(AppVerifyConfig {
            fqdn,
            resolver_ip: "1.1.1.1".to_string(),
            expected_ip: ansible_host.to_string(),
            is_tailnet: false,
        });
    }

    None
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

    // ── app_verify_config ─────────────────────────────────────────────────────

    fn make_config(toml_str: &str) -> Config {
        Config::from_toml_str(toml_str)
    }

    #[test]
    fn test_app_verify_config_tailnet() {
        let config = make_config(
            r#"
domain = "example.com"
paperless_subdomain = "paperless"
paperless_tailscale_ip = "100.64.1.2"
"#,
        );
        let vc = app_verify_config("paperless", "example.com", "1.2.3.4", &config, false).unwrap();
        assert_eq!(vc.fqdn, "paperless.example.com");
        assert_eq!(vc.resolver_ip, "100.64.1.2");
        assert_eq!(vc.expected_ip, "100.64.1.2");
        assert!(vc.is_tailnet);
    }

    #[test]
    fn test_app_verify_config_public_opt_in() {
        let config = make_config(
            r#"
domain = "example.com"
freshrss_subdomain = "rss"
"#,
        );
        let vc =
            app_verify_config("freshrss", "example.com", "203.0.113.10", &config, true).unwrap();
        assert_eq!(vc.fqdn, "rss.example.com");
        assert_eq!(vc.resolver_ip, "1.1.1.1");
        assert_eq!(vc.expected_ip, "203.0.113.10");
        assert!(!vc.is_tailnet);
    }

    #[test]
    fn test_app_verify_config_public_opt_out() {
        let config = make_config(
            r#"
domain = "example.com"
freshrss_subdomain = "rss"
"#,
        );
        // verify_public = false → None for public apps
        assert!(
            app_verify_config("freshrss", "example.com", "203.0.113.10", &config, false).is_none()
        );
    }

    #[test]
    fn test_app_verify_config_no_subdomain() {
        let config = make_config(r#"domain = "example.com""#);
        assert!(app_verify_config("paperless", "example.com", "1.2.3.4", &config, true).is_none());
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
