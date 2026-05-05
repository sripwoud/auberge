use crate::config::Config;
use eyre::Result;
use std::net::IpAddr;

/// The outcome of a DNS A-record verification.
#[derive(Debug, PartialEq)]
pub enum VerifyOutcome {
    /// The A record exists and includes the expected IP.
    Match,
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
    resolver_ip: String,
}

impl HickoryLookup {
    pub fn new(resolver_ip: &str) -> Self {
        Self {
            resolver_ip: resolver_ip.to_string(),
        }
    }
}

impl DnsLookup for HickoryLookup {
    fn lookup_ipv4(&self, fqdn: &str) -> Result<Vec<IpAddr>> {
        use hickory_resolver::{
            TokioResolver,
            config::{NameServerConfig, ResolverConfig, ResolverOpts},
            net::runtime::TokioRuntimeProvider,
        };
        use std::str::FromStr;

        let addr: IpAddr = IpAddr::from_str(&self.resolver_ip)
            .map_err(|e| eyre::eyre!("Invalid resolver IP '{}': {}", self.resolver_ip, e))?;

        let ns = NameServerConfig::udp(addr);
        let config = ResolverConfig::from_parts(None, vec![], vec![ns]);
        let mut opts = ResolverOpts::default();
        opts.attempts = 2;

        // Use block_in_place to run async resolver code from within a synchronous
        // function that's executing on a multi-threaded tokio runtime.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let resolver =
                    TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
                        .with_options(opts)
                        .build()
                        .map_err(|e| eyre::eyre!("Failed to build DNS resolver: {}", e))?;

                // Append trailing dot to query the name as a FQDN, preventing
                // the resolver from appending any search-domain suffix.
                let fqdn_dot = if fqdn.ends_with('.') {
                    fqdn.to_string()
                } else {
                    format!("{fqdn}.")
                };

                match resolver.lookup_ip(fqdn_dot.as_str()).await {
                    Ok(lookup) => {
                        let ips: Vec<IpAddr> =
                            lookup.iter().filter(|a: &IpAddr| a.is_ipv4()).collect();
                        Ok(ips)
                    }
                    Err(e) if e.is_no_records_found() => Ok(vec![]),
                    Err(e) => Err(eyre::eyre!("DNS lookup error: {}", e)),
                }
            })
        })
    }
}

/// Compare the DNS lookup result against `expected_ip` and return the outcome.
pub fn verify_a_record<L: DnsLookup>(
    lookup: &L,
    fqdn: &str,
    expected_ip: &str,
) -> Result<VerifyOutcome> {
    match lookup.lookup_ipv4(fqdn) {
        Ok(ips) if ips.is_empty() => Ok(VerifyOutcome::NxDomain),
        Ok(ips) => {
            let ip_strs: Vec<String> = ips.iter().map(|ip| ip.to_string()).collect();
            if ip_strs.iter().any(|ip| ip == expected_ip) {
                Ok(VerifyOutcome::Match)
            } else {
                Ok(VerifyOutcome::Mismatch { got: ip_strs })
            }
        }
        Err(e) => Err(e),
    }
}

/// Returns `true` if `ip` is in the Tailscale CGNAT range (100.64.0.0/10).
pub fn is_tailscale_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let octets = addr.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
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
    if let Some(tailscale_ip) = config.get(&tailscale_key).filter(|v| !v.is_empty()) {
        if is_tailscale_ip(&tailscale_ip) {
            return Some(AppVerifyConfig {
                fqdn,
                resolver_ip: tailscale_ip.clone(),
                expected_ip: tailscale_ip,
                is_tailnet: true,
            });
        }
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
pub fn format_dns_error(fqdn: &str, resolver_ip: &str, expected_ip: &str, outcome: &VerifyOutcome) -> String {
    match outcome {
        VerifyOutcome::Mismatch { got } => format!(
            "DNS mismatch for {fqdn}: queried {resolver_ip}, expected {expected_ip}, got [{}]",
            got.join(", ")
        ),
        VerifyOutcome::NxDomain => format!(
            "DNS check failed for {fqdn}: queried {resolver_ip}, expected {expected_ip}, got NXDOMAIN (name not found)"
        ),
        VerifyOutcome::Match => String::new(),
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
        let lookup = MockLookup::new()
            .with_found(fqdn, vec![ip.parse::<Ipv4Addr>().unwrap()]);
        assert_eq!(
            verify_a_record(&lookup, fqdn, ip).unwrap(),
            VerifyOutcome::Match
        );
    }

    #[test]
    fn test_verify_match_public() {
        let ip = "203.0.113.10";
        let fqdn = "app.example.com";
        let lookup = MockLookup::new()
            .with_found(fqdn, vec![ip.parse::<Ipv4Addr>().unwrap()]);
        assert_eq!(
            verify_a_record(&lookup, fqdn, ip).unwrap(),
            VerifyOutcome::Match
        );
    }

    #[test]
    fn test_verify_mismatch() {
        let fqdn = "app.example.com";
        let actual_ip = "203.0.113.99";
        let lookup = MockLookup::new()
            .with_found(fqdn, vec![actual_ip.parse::<Ipv4Addr>().unwrap()]);
        let outcome = verify_a_record(&lookup, fqdn, "203.0.113.10").unwrap();
        assert_eq!(
            outcome,
            VerifyOutcome::Mismatch {
                got: vec![actual_ip.to_string()]
            }
        );
    }

    #[test]
    fn test_verify_nxdomain() {
        let fqdn = "missing.example.com";
        let lookup = MockLookup::new().with_nxdomain(fqdn);
        assert_eq!(
            verify_a_record(&lookup, fqdn, "203.0.113.10").unwrap(),
            VerifyOutcome::NxDomain
        );
    }

    #[test]
    fn test_verify_lookup_error_propagated() {
        let fqdn = "app.example.com";
        let lookup = MockLookup::new().with_error(fqdn, "timeout");
        assert!(verify_a_record(&lookup, fqdn, "203.0.113.10").is_err());
    }

    // ── is_tailscale_ip ───────────────────────────────────────────────────────

    #[test]
    fn test_is_tailscale_ip_true() {
        assert!(is_tailscale_ip("100.64.0.1"));
        assert!(is_tailscale_ip("100.100.200.1"));
        assert!(is_tailscale_ip("100.127.255.255"));
    }

    #[test]
    fn test_is_tailscale_ip_false() {
        assert!(!is_tailscale_ip("100.128.0.1")); // just outside range
        assert!(!is_tailscale_ip("192.168.1.1"));
        assert!(!is_tailscale_ip("203.0.113.10"));
        assert!(!is_tailscale_ip("not-an-ip"));
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
            &VerifyOutcome::Mismatch {
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
            &VerifyOutcome::NxDomain,
        );
        assert!(msg.contains("app.example.com"));
        assert!(msg.contains("1.1.1.1"));
        assert!(msg.contains("NXDOMAIN"));
    }
}
