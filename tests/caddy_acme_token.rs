//! The parent domain's ACME token never lands on the agent tier's Host.
//!
//! Caddy answers DNS-01 with whatever `CLOUDFLARE_DNS_API_TOKEN` its systemd
//! drop-in carries. A Cloudflare API token is zone-scoped and not
//! record-scoped, so the smallest token able to complete a challenge for the
//! parent domain can also rewrite its MX and every other Host's records
//! (ADR-0068). `ruche` is assumed compromisable (ADR-0054), which makes "which
//! token is in that drop-in" a security property of the deploy and not a
//! configuration detail.
//!
//! Three things hold it:
//!
//! - the drop-in reads `caddy_dns_api_token`, the indirection, and not a
//!   config key directly — a template that reads `cloudflare_dns_api_token`
//!   cannot be pointed anywhere;
//! - the role defaults that indirection to the parent domain's token, so the
//!   19 Hosts that are not the agent tier do not move;
//! - `infrastructure.yml` resolves it to the agent tier's token on the Host
//!   whose config answers the agent tier's serving gate, and to the parent
//!   domain's everywhere else.
//!
//! The third is asserted by **evaluating** the playbook's expression against a
//! written-out context, both ways round, because the failure that matters is
//! not a missing branch but a branch that resolves to the wrong token — which
//! reads as a working deploy right up until the token leaks.

use std::collections::BTreeMap;
use std::fs;

use minijinja::value::Value as JValue;
use minijinja::{Environment, UndefinedBehavior};
use serde_yaml::Value;

mod common;

use common::{
    defaults, field, parse_yaml, playbooks_dir, registry_keys, role_dir, role_template_files,
};

/// The role variable the drop-in reads: caddy's own name for "the token I
/// answer DNS-01 with", which is what makes it pointable.
const INDIRECTION: &str = "caddy_dns_api_token";

/// The parent domain's token, scoped to the parent zone and to be found on
/// every Host but one.
const FLEET_KEY: &str = "cloudflare_dns_api_token";

/// The agent tier's token, scoped to `agents_domain` alone (ADR-0068).
const AGENT_KEY: &str = "agents_cloudflare_dns_api_token";

/// The key whose answer marks a Host as serving the agent tier (ADR-0051):
/// config alone answers it, and a blank value is no answer.
const GATE_KEY: &str = "aoe_subdomain";

/// Written-out token values, so an edit that swaps which branch yields which
/// has to move an assertion too (ADR-0046).
const FLEET_TOKEN: &str = "parent-zone-token";
const AGENT_TOKEN: &str = "agent-zone-token";

fn env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env
}

/// The `vars:` the caddy entry of `infrastructure.yml` binds.
fn caddy_entry_vars() -> BTreeMap<String, String> {
    let plays = parse_yaml(&playbooks_dir().join("infrastructure.yml"));
    let entries = plays
        .as_sequence()
        .and_then(|plays| plays.first())
        .and_then(|play| play.get("roles"))
        .and_then(Value::as_sequence)
        .expect("infrastructure.yml must hold a play with a roles list");
    let entry = entries
        .iter()
        .filter_map(Value::as_mapping)
        .find(|entry| field(entry, "role").and_then(Value::as_str) == Some("caddy"))
        .expect("infrastructure.yml must run the caddy role");
    field(entry, "vars")
        .and_then(Value::as_mapping)
        .map(|vars| {
            vars.iter()
                .filter_map(|(name, expr)| {
                    Some((name.as_str()?.to_string(), expr.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The token `infrastructure.yml` hands caddy, given what config answers.
///
/// `gate` is `None` where the key has no answer at all, which is every Host but
/// the agent tier's — the shape ansible leaves behind, and the reason the
/// expression has to survive an undefined name rather than an empty one.
fn resolved_token(gate: Option<&str>) -> String {
    let expression = caddy_entry_vars().remove(INDIRECTION).unwrap_or_else(|| {
        panic!("infrastructure.yml must bind `{INDIRECTION}` on the caddy entry")
    });

    let mut context: BTreeMap<&str, JValue> = BTreeMap::from([
        (FLEET_KEY, JValue::from(FLEET_TOKEN)),
        (AGENT_KEY, JValue::from(AGENT_TOKEN)),
    ]);
    if let Some(answer) = gate {
        context.insert(GATE_KEY, JValue::from(answer));
    }

    env()
        .render_str(&expression, JValue::from_serialize(&context))
        .unwrap_or_else(|e| panic!("`{INDIRECTION}` must evaluate: {e}\n  {expression}"))
        .trim()
        .to_string()
}

// ── The drop-in reads the indirection ─────────────────────────────────────

/// The one template that writes the token into caddy's environment, and its
/// body. A drop-in that names the config key directly is the shape this whole
/// file exists to prevent: nothing can point it.
#[test]
fn test_the_env_dropin_reads_the_indirection_and_not_a_config_key() {
    let templates: Vec<String> = role_template_files("caddy")
        .iter()
        .map(|path| fs::read_to_string(path).expect("a caddy template must be readable"))
        .filter(|body| body.contains("CLOUDFLARE_DNS_API_TOKEN"))
        .collect();
    assert_eq!(
        templates.len(),
        1,
        "exactly one caddy template may write CLOUDFLARE_DNS_API_TOKEN; \
         a second one is a second answer to which token this Host holds"
    );
    let body = &templates[0];
    assert!(
        body.contains(INDIRECTION),
        "caddy's env drop-in must read `{INDIRECTION}`, so a Host serving another \
         zone can be pointed at another token:\n{body}"
    );
    assert!(
        !body.contains(&format!("{{{{ {FLEET_KEY} }}}}")),
        "caddy's env drop-in must not read `{FLEET_KEY}` directly — that is the \
         parent domain's token, and it cannot be redirected:\n{body}"
    );
}

/// The default is the parent domain's token, resolved through the role's own
/// defaults. Every Host but the agent tier's takes it, so a default that moved
/// would re-point the whole fleet.
#[test]
fn test_the_indirection_defaults_to_the_parent_domains_token() {
    let raw = fs::read_to_string(role_dir("caddy").join("defaults/main.yml"))
        .expect("caddy defaults must exist");
    let parsed: Value = serde_yaml::from_str(&raw).expect("caddy defaults must parse");
    assert_eq!(
        parsed[INDIRECTION].as_str(),
        Some(format!("{{{{ {FLEET_KEY} }}}}").as_str()),
        "`{INDIRECTION}` must default to the parent domain's token"
    );
    assert!(
        defaults("caddy").contains_key(INDIRECTION),
        "the shared defaults read must see `{INDIRECTION}`; the unit scan resolves \
         the drop-in's value through it"
    );
}

// ── What each Host is handed ──────────────────────────────────────────────

/// The property: a Host answering the agent tier's gate gets the agent tier's
/// token, and nothing else.
#[test]
fn test_the_agent_tiers_host_is_handed_the_agent_tiers_token() {
    assert_eq!(
        resolved_token(Some("essaim")),
        AGENT_TOKEN,
        "a Host serving the agent tier must answer DNS-01 with the token scoped \
         to `agents_domain` alone"
    );
}

/// The other direction, which is the fleet: 19 Hosts do not move.
#[test]
fn test_a_host_that_does_not_serve_the_agent_tier_keeps_the_parent_token() {
    assert_eq!(
        resolved_token(None),
        FLEET_TOKEN,
        "every other Host answers DNS-01 for the parent domain and must keep its token"
    );
}

/// A blank answer is no answer (ADR-0051), which is how `[hosts.<name>]`
/// withdraws a fleet-wide value for one Host. Reading it as an answer would
/// hand the agent tier's token to a Host that withdrew the gate.
#[test]
fn test_a_blank_gate_is_not_an_answer() {
    for blank in ["", "   "] {
        assert_eq!(
            resolved_token(Some(blank)),
            FLEET_TOKEN,
            "a blank `{GATE_KEY}` withdraws the gate; it does not answer it"
        );
    }
}

/// Both keys the expression can resolve to are keys an operator can set, or one
/// branch of it is unreachable by any config.
#[test]
fn test_both_tokens_are_registry_keys() {
    let registry = registry_keys();
    for key in [FLEET_KEY, AGENT_KEY, GATE_KEY] {
        assert!(
            registry.contains(key),
            "`{key}` must be in ansible/keys.yml, or no config can answer it"
        );
    }
}
