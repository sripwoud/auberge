//! The agent dashboard is reachable from the tailnet, behind two factors, and
//! its bearer token is not written to disk.
//!
//! `aoe serve` is the one App in the fleet that can publish itself. `--remote`
//! spawns a public Tailscale Funnel or Cloudflare quick tunnel and hands out an
//! internet-reachable URL, which would take a Host assumed compromisable
//! (ADR-0054) and give it public ingress — past Caddy, past the tailnet, past
//! the ACL. Nothing else in the repo has a flag that undoes a Playbook Meta's
//! `tailnet_only: true`, so nothing else needs a fence saying it is absent.
//!
//! Three more properties, each of which fails silently or fails late:
//!
//! - **the DNS-rebinding gate has to be told the proxied name.** aoe rejects a
//!   `Host` header it was not given, so a vhost without `--allowed-host` serves
//!   a certificate, terminates TLS, and then 421s every request;
//! - **the passphrase is the second factor, not the only one.** `--auth
//!   passphrase` is *tokenless* upstream — it removes the URL token rather than
//!   adding to it, and breaks local TUI attach. Default token auth plus
//!   `--passphrase` is the two-factor pairing (token in the URL, passphrase on
//!   the login page) that upstream documents;
//! - **that token rides in the query string**, refreshed every four hours, so
//!   Caddy's access log for this vhost has to drop the query. Caddy redacts
//!   `Authorization` and `Cookie` on its own and nothing else.
//!
//! ## The log filter is applied, not matched
//!
//! The scrub is asserted by extracting the filter's own regexp out of the
//! rendered vhost and running it over a request URI carrying a token. A text
//! match cannot tell the working spelling from `"\\?.*$"`, which Caddy's
//! Caddyfile lexer hands to Go as "an optional literal backslash" — it matches
//! at offset zero and logs `"uri": ""`, dropping the path along with the token.
//! Both spellings scrub the secret; only one leaves a usable log.

use std::collections::BTreeMap;
use std::fs;

use minijinja::value::Value as JValue;
use minijinja::{Environment, UndefinedBehavior};

mod common;

use common::{parse_yaml, playbooks_dir, role_dir};

/// The dashboard's FQDN and the address it is served on, written out rather
/// than resolved from the role's defaults: an expectation read back from what
/// it is checking asserts nothing (ADR-0046).
const DOMAIN: &str = "essaim.agents-example.com";
const TAILNET_ADDRESS: &str = "100.64.0.9";
const LOOPBACK: &str = "127.0.0.1";
const PORT: &str = "8080";

/// The flags that would publish the dashboard past Caddy and past the tailnet.
/// `--remote` prefers a Tailscale Funnel and falls back to a Cloudflare quick
/// tunnel; the other three steer which tunnel, and only make sense with it.
const TUNNEL_FLAGS: &[&str] = &[
    "--remote",
    "--tunnel-name",
    "--tunnel-url",
    "--no-tailscale",
];

/// The flags that would take the login wall down.
const AUTHLESS_FLAGS: &[&str] = &["--no-auth", "--read-only"];

fn jinja() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env
}

fn render(template: &str, context: &[(&str, &str)]) -> String {
    let body = fs::read_to_string(role_dir("aoe").join("templates").join(template))
        .unwrap_or_else(|e| panic!("aoe/templates/{template} must exist: {e}"));
    let map: BTreeMap<&str, &str> = context.iter().copied().collect();
    jinja()
        .render_str(&body, JValue::from_serialize(&map))
        .unwrap_or_else(|e| panic!("aoe/templates/{template} must render: {e}"))
}

/// The unit as ansible will write it, with every name the role's defaults and
/// the injected Memory Budget supply.
fn unit() -> String {
    render(
        "aoe.service.j2",
        &[
            ("aoe_binary_path", "/usr/local/bin/aoe"),
            ("aoe_bind_host", LOOPBACK),
            ("aoe_port", PORT),
            ("aoe_domain", DOMAIN),
            ("aoe_home", "/home/agent"),
            ("aoe_env_path", "/home/agent/.config/aoe/serve.env"),
            ("aoe_memory_high", "4G"),
        ],
    )
}

/// The vhost as ansible will write it.
fn vhost() -> String {
    render(
        "aoe.caddyfile.j2",
        &[
            ("aoe_domain", DOMAIN),
            ("aoe_tailscale_ip", TAILNET_ADDRESS),
            ("aoe_bind_host", LOOPBACK),
            ("aoe_port", PORT),
        ],
    )
}

/// One directive's value out of a rendered unit — the last assignment wins, as
/// systemd reads it. Absent is a hard stop: every assertion below is about a
/// directive that has to be there.
fn directive(key: &str) -> String {
    let body = unit();
    let value = body
        .lines()
        .filter_map(|line| line.trim().split_once('='))
        .filter(|(name, _)| *name == key)
        .map(|(_, value)| value.to_string())
        .next_back();
    value.unwrap_or_else(|| panic!("aoe.service must set `{key}`:\n{body}"))
}

/// `aoe serve`'s argv, past the binary.
fn serve_argv() -> Vec<String> {
    let exec = directive("ExecStart");
    let mut argv = exec.split_whitespace().map(str::to_string);
    let binary = argv.next().expect("ExecStart names a binary");
    assert!(
        binary.ends_with("/aoe"),
        "ExecStart must exec the aoe binary, not {binary}"
    );
    let argv: Vec<String> = argv.collect();
    assert_eq!(
        argv.first().map(String::as_str),
        Some("serve"),
        "the unit runs the dashboard, so its subcommand is `serve`: {argv:?}"
    );
    argv
}

/// The value of a repeatable `--flag value` pair.
fn flag_value(flag: &str) -> Option<String> {
    let argv = serve_argv();
    argv.iter()
        .position(|arg| arg == flag)
        .and_then(|at| argv.get(at + 1).cloned())
}

// ── What the dashboard is not ─────────────────────────────────────────────

/// The property that makes every other one worth having. `--remote` hands out
/// an internet-reachable URL from a Host with no public ingress by design.
#[test]
fn test_serve_never_opens_a_public_tunnel() {
    let argv = serve_argv();
    for flag in TUNNEL_FLAGS {
        assert!(
            !argv.iter().any(|arg| arg == flag),
            "`{flag}` publishes the dashboard outside the tailnet, past Caddy and \
             past the ACL, from the one Host the fleet assumes is compromisable \
             (ADR-0054): {argv:?}"
        );
    }
}

#[test]
fn test_serve_keeps_its_login_wall() {
    let argv = serve_argv();
    for flag in AUTHLESS_FLAGS {
        assert!(
            !argv.iter().any(|arg| arg == flag),
            "`{flag}` is not what a phone control plane wants: {argv:?}"
        );
    }
    assert_eq!(
        flag_value("--auth").as_deref(),
        Some("token"),
        "token auth is what keeps the URL token beside the passphrase; \
         `--auth passphrase` is tokenless upstream and drops one of the two factors"
    );
}

/// The passphrase reaches the process through the environment. On the command
/// line it would be readable out of `/proc` by every process on a Host running
/// unattended agents.
#[test]
fn test_the_passphrase_is_not_on_the_command_line() {
    let argv = serve_argv();
    assert!(
        !argv.iter().any(|arg| arg == "--passphrase"),
        "the passphrase must not be an argv entry: {argv:?}"
    );
    assert!(
        !unit().contains("aoe_passphrase"),
        "the unit file must not carry the passphrase; it is world-readable at 0644"
    );

    let env_file = directive("EnvironmentFile");
    assert!(
        env_file.ends_with("serve.env"),
        "the unit must read its environment from the role's own file, not {env_file}"
    );
    // Single-quoted, and asserted on a value carrying what systemd would
    // otherwise eat: an unquoted `p\ss` reaches the process as `pss`, and every
    // device is locked out with nothing anywhere reporting why. Upstream's own
    // default passphrase is four words, so spaces are the normal case.
    let rendered = render("serve.env.j2", &[("aoe_passphrase", "four word p\\ss #1")]);
    let assignment = rendered
        .lines()
        .find(|line| line.starts_with("AOE_SERVE_PASSPHRASE"))
        .expect("the environment file must set the variable aoe reads the passphrase from");
    assert_eq!(
        assignment, "AOE_SERVE_PASSPHRASE='four word p\\ss #1'",
        "the value must be single-quoted; systemd consumes a backslash in an \
         unquoted value as an escape and strips trailing whitespace"
    );
}

// ── What the reverse proxy needs it told ──────────────────────────────────

/// aoe runs a DNS-rebinding gate: it trusts loopback, routable IP literals and
/// its own `--host`, and rejects any other `Host` header. Serving it by name
/// through Caddy means naming that host, or every proxied request 421s behind a
/// valid certificate.
#[test]
fn test_serve_is_told_the_name_and_origin_caddy_proxies() {
    assert_eq!(
        flag_value("--allowed-host").as_deref(),
        Some(DOMAIN),
        "without the proxied hostname the rebinding gate rejects every request \
         Caddy forwards"
    );
    assert_eq!(
        flag_value("--allowed-origin").as_deref(),
        Some(format!("https://{DOMAIN}").as_str()),
        "the browser Origin is the https one Caddy terminates, not the loopback \
         one aoe binds"
    );
    assert!(
        serve_argv().iter().any(|arg| arg == "--behind-proxy"),
        "Caddy terminates TLS, so cookies must be set Secure and the forwarded \
         client IP trusted from loopback"
    );
}

/// Loopback only. A bind on the tailnet address would be a second way in that
/// Caddy neither terminates TLS for nor logs.
#[test]
fn test_serve_binds_loopback_and_caddy_binds_the_tailnet() {
    assert_eq!(flag_value("--host").as_deref(), Some(LOOPBACK));
    assert_eq!(flag_value("--port").as_deref(), Some(PORT));

    let vhost = vhost();
    assert!(
        vhost.contains(&format!("bind {TAILNET_ADDRESS}")),
        "the vhost must bind the Host's tailnet address and nothing else:\n{vhost}"
    );
    assert!(
        vhost.contains(&format!("reverse_proxy {LOOPBACK}:{PORT}")),
        "the vhost must proxy the loopback listener aoe binds:\n{vhost}"
    );
}

// ── What the access log keeps ─────────────────────────────────────────────

/// The `request>uri` filter the vhost declares, as `(pattern, replacement)`.
fn uri_filter() -> (String, String) {
    let vhost = vhost();
    let line = vhost
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("request>uri "))
        .unwrap_or_else(|| {
            panic!(
                "the vhost's access log must filter `request>uri`, where the token rides:\n{vhost}"
            )
        })
        .to_string();
    let mut quoted = line.split('"').skip(1).step_by(2);
    let pattern = quoted
        .next()
        .unwrap_or_else(|| panic!("the `request>uri` filter must carry a quoted pattern: {line}"));
    let replacement = quoted.next().unwrap_or("");
    assert!(
        line.contains("regexp"),
        "the filter must be a `regexp` one; `query` deletes named parameters one \
         at a time and cannot drop a query string whole: {line}"
    );
    (pattern.to_string(), replacement.to_string())
}

/// The filter, applied. `caddy adapt` hands the pattern to Go's regexp engine
/// verbatim — the Caddyfile lexer does not collapse `\\` inside quotes — so the
/// only way to know what it does to a URI is to run it over one.
#[test]
fn test_the_access_log_drops_the_query_and_keeps_the_path() {
    let (pattern, replacement) = uri_filter();
    let regex = regex::Regex::new(&pattern)
        .unwrap_or_else(|e| panic!("the filter's pattern must compile as a regexp: {e}"));

    let logged = regex.replace_all(
        "/dash?token=SUPERSECRET&project=auberge",
        replacement.as_str(),
    );
    assert!(
        !logged.contains("SUPERSECRET"),
        "the bearer token rides in `?token=` and must not reach the log; \
         `{pattern}` left {logged}"
    );
    assert_eq!(
        logged, "/dash",
        "the path has to survive the scrub, or the access log stops saying which \
         request was served; `{pattern}` left {logged:?}"
    );
    assert_eq!(
        regex.replace_all("/dash", replacement.as_str()),
        "/dash",
        "a request with no query must log unchanged; `{pattern}` rewrites one that \
         carries no secret"
    );
}

/// The log is a file of its own, so the scrub applies to what is written rather
/// than to a subset of it.
#[test]
fn test_the_vhost_logs_to_its_own_file_through_the_filter() {
    let vhost = vhost();
    assert!(
        vhost.contains(&format!("output file /var/log/caddy/{DOMAIN}.log")),
        "the vhost must write its own access log:\n{vhost}"
    );
    assert!(
        vhost.contains("format filter"),
        "an unfiltered format logs the URI as it arrived, token and all:\n{vhost}"
    );
}

// ── What the Meta declares ────────────────────────────────────────────────

/// A user unit is invisible to `systemctl` without `--user`, and a Serving Unit
/// in the user manager hooks `default.target` rather than `multi-user.target`.
/// Both are what the fleet's unit fences read this App through.
#[test]
fn test_the_dashboard_is_a_lingering_user_unit() {
    assert_eq!(
        directive("WantedBy"),
        "default.target",
        "the user manager has no `multi-user.target`; a unit hooked there never starts"
    );
    let tasks = fs::read_to_string(role_dir("aoe").join("tasks/main.yml"))
        .expect("the aoe role must have tasks");
    assert!(
        tasks.contains("loginctl enable-linger"),
        "without lingering the user manager is torn down at logout, which takes \
         the dashboard down on every SSH disconnect"
    );

    let meta = parse_yaml(&playbooks_dir().join("aoe.meta.yml"));
    assert_eq!(meta["units"][0]["name"].as_str(), Some("aoe"));
    assert_eq!(meta["units"][0]["scope"].as_str(), Some("user"));
    assert_eq!(meta["tailnet_only"].as_bool(), Some(true));
}
