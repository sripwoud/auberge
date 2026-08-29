//! The tailnet keeps a relay when the control-plane Host does not.
//!
//! `derp.urls` was `[]`, so the embedded DERP server on the auberge Host was
//! the tailnet's only relay region. Two peers that cannot reach each other
//! directly — a laptop and a phone both behind hard NAT — relay through it, and
//! that traffic dies with the box, having never needed it. The control plane
//! being a single point of failure for enrollment is the deliberate cost of
//! self-hosting it (ADR-0049); being one for peer traffic is not (#709).
//!
//! The fallback is Tailscale's public DERP map, merged in as extra regions
//! beside the embedded one. Relayed traffic is end-to-end encrypted WireGuard
//! that a DERP node forwards without being able to read, so this borrows their
//! relay fleet's availability, not their trust.
//!
//! ## Why the fetch regime is fenced, not just the URL
//!
//! Headscale reads the map in two places, and they fail differently.
//! `hscontrol/derp.GetDERPMap` returns on the *first* URL that fails to fetch,
//! with no per-URL tolerance. `hscontrol/app.go` calls it once from `Serve()`
//! and returns the error, so a URL that cannot be fetched at startup stops the
//! control plane from starting. It calls it again from the update ticker, there
//! wrapped in a backoff retry that logs and carries on.
//!
//! So a non-empty `urls` list buys freshness only through the survivable path,
//! and only when the ticker runs: with `auto_update_enabled: false` the map is
//! whatever was fetched at the last restart, and a region Tailscale retires
//! stays in it until something restarts headscale. The two settings are one
//! decision, which is why they move together here — auto-update is on exactly
//! when there is a remote source to update from, and off when the map is
//! entirely local and nothing could change under it.
//!
//! ## Expectations are literals, not the defaults read back
//!
//! Every expected value below is written out. Reading it from
//! `defaults/main.yml` instead would compare the render against the same file
//! the render is fed from, so editing the default would move both sides at once
//! and the assertion would hold for any value — ADR-0046's failure, where a
//! fence "pins only what the scan found, so a scan that found less passed
//! vacuously". A literal is what makes changing the default a decision that has
//! to be made twice.

use std::collections::BTreeMap;
use std::fs;

use minijinja::{Environment, UndefinedBehavior};
use serde_yaml::Value;

mod common;

use common::{parse_yaml, repo, role_dir};

/// Tailscale's published DERP map — the fleet the fallback borrows.
const TAILSCALE_DERP_MAP: &str = "https://controlplane.tailscale.com/derpmap/default";

/// The embedded server's region. Headscale inserts it *after* merging the
/// remote maps (`hscontrol/app.go`), so a remote map claiming the same ID
/// cannot displace it.
const EMBEDDED_REGION_ID: u64 = 999;

/// How often the ticker re-fetches. Upstream's own default is `3h`; the fleet
/// this serves changes far more slowly than that.
const UPDATE_FREQUENCY: &str = "24h";

fn headscale_role_dir() -> std::path::PathBuf {
    role_dir("headscale")
}

fn role_defaults() -> BTreeMap<String, Value> {
    let raw = fs::read_to_string(headscale_role_dir().join("defaults/main.yml"))
        .expect("headscale defaults must exist");
    serde_yaml::from_str(&raw).expect("headscale defaults must parse")
}

/// Names the operator supplies through the Key Registry rather than the role,
/// seeded rather than defaulted. `headscale_subdomain` has a role default today
/// and #710 removes it — the gate it feeds can never be false while it does —
/// so reading it from `defaults/main.yml` would make this fence fail on that
/// change for a reason that has nothing to do with relays.
const KEY_REGISTRY_ANSWERS: &[(&str, &str)] =
    &[("domain", "example.com"), ("headscale_subdomain", "hs")];

/// The App Version the Meta pins, injected into every run as `<app>_version`
/// (ADR-0017). Seeded here because the defaults reference it and nothing in
/// `defaults/main.yml` declares it.
fn headscale_version() -> String {
    let meta = parse_yaml(&repo().join("ansible/playbooks/headscale.meta.yml"));
    meta["version"]["value"]
        .as_str()
        .expect("headscale.meta.yml must pin a version")
        .to_string()
}

/// Render the way `ansible.builtin.template` will.
///
/// `trim_blocks` is on because ansible defaults it on where jinja2's own
/// default is off, which eats the newline after a block tag and can join the
/// next directive onto the previous line (#582).
///
/// Undefined is strict because minijinja's default is lenient: an unknown name
/// renders empty and every assertion downstream still passes over a config
/// ansible would have died producing. Lenient, this file resolved
/// `headscale_release_url` to `…/download/v/headscale__linux_amd64` without
/// complaint.
fn jinja() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env
}

/// Role defaults reference each other (`headscale_server_url` is built from
/// `headscale_domain`, which is built from `domain`). Ansible resolves those
/// lazily at use; resolve them to a fixpoint here so the render sees the values
/// a deploy would, not the templates they are written as.
///
/// Only string values are resolved. Every non-string default is a literal
/// today, and one that stopped being one would be skipped silently, so the
/// resolver refuses rather than skips.
fn deploy_context(overrides: &[(&str, Value)]) -> BTreeMap<String, minijinja::Value> {
    let mut vars = role_defaults();
    for (key, value) in KEY_REGISTRY_ANSWERS {
        vars.insert((*key).to_string(), Value::from(*value));
    }
    vars.insert("headscale_version".into(), Value::from(headscale_version()));
    for (name, value) in overrides {
        vars.insert((*name).to_string(), value.clone());
    }

    let env = jinja();
    for _ in 0..8 {
        let resolved: BTreeMap<String, minijinja::Value> = vars
            .iter()
            .map(|(name, value)| (name.clone(), minijinja::Value::from_serialize(value)))
            .collect();
        let mut changed = false;
        for (name, value) in vars.iter_mut() {
            let Some(text) = value.as_str() else {
                assert!(
                    !serde_yaml::to_string(value).unwrap().contains("{{"),
                    "{name} holds a template this resolver only reaches inside strings"
                );
                continue;
            };
            if !text.contains("{{") {
                continue;
            }
            let rendered = env
                .render_str(text, &resolved)
                .unwrap_or_else(|e| panic!("role default {name} must render: {e}"));
            *value = Value::from(rendered);
            changed = true;
        }
        if !changed {
            return resolved;
        }
    }
    panic!("headscale role defaults did not resolve to a fixpoint");
}

/// The config headscale reads, parsed. Asserting on the parsed document rather
/// than the rendered text is what makes `urls` a list claim instead of a
/// substring one.
fn render_config(overrides: &[(&str, Value)]) -> Value {
    let template =
        fs::read_to_string(headscale_role_dir().join("templates/headscale-config.yaml.j2"))
            .expect("headscale-config.yaml.j2 must exist");
    let rendered = jinja()
        .render_str(&template, deploy_context(overrides))
        .expect("headscale-config.yaml.j2 must render");
    serde_yaml::from_str(&rendered)
        .unwrap_or_else(|e| panic!("rendered headscale config must parse as YAML: {e}\n{rendered}"))
}

fn derp(config: &Value) -> &Value {
    &config["derp"]
}

fn urls(config: &Value) -> Vec<String> {
    derp(config)["urls"]
        .as_sequence()
        .expect("derp.urls must render as a list")
        .iter()
        .map(|url| {
            url.as_str()
                .expect("every derp.urls entry must be a string")
                .to_string()
        })
        .collect()
}

#[test]
fn test_the_shipped_default_relays_through_the_public_tailscale_map() {
    assert_eq!(urls(&render_config(&[])), vec![TAILSCALE_DERP_MAP]);
}

/// Which region a client calls home is measured, not configured — it probes
/// every region and picks by latency. What the config can state is that the
/// self-hosted one is still offered, under the ID headscale merges last.
#[test]
fn test_the_embedded_region_is_still_offered_beside_the_remote_map() {
    let config = render_config(&[]);
    assert_eq!(derp(&config)["server"]["enabled"], Value::from(true));
    assert_eq!(
        derp(&config)["server"]["region_id"],
        Value::from(EMBEDDED_REGION_ID)
    );
}

#[test]
fn test_a_remote_source_is_kept_fresh() {
    let config = render_config(&[]);
    assert_eq!(derp(&config)["auto_update_enabled"], Value::from(true));
    assert_eq!(
        derp(&config)["update_frequency"],
        Value::from(UPDATE_FREQUENCY)
    );
}

/// The template walks the list rather than interpolating it, so a second entry
/// renders as a second YAML item and not as the debug form of a list.
#[test]
fn test_every_entry_in_the_list_reaches_the_config() {
    let extra = "https://derp.example.org/derpmap.json";
    let config = render_config(&[(
        "headscale_derp_urls",
        Value::from(vec![TAILSCALE_DERP_MAP, extra]),
    )]);
    assert_eq!(urls(&config), vec![TAILSCALE_DERP_MAP, extra]);
}

/// The escape hatch, and the config as it stood before #709: nothing to fetch
/// at startup, so nothing a fetch failure can stop from starting.
#[test]
fn test_an_empty_list_leaves_nothing_to_fetch_and_nothing_to_update() {
    let config = render_config(&[("headscale_derp_urls", Value::from(Vec::<String>::new()))]);
    assert!(urls(&config).is_empty());
    assert_eq!(derp(&config)["auto_update_enabled"], Value::from(false));
    assert!(
        derp(&config).get("update_frequency").is_none(),
        "an update frequency with nothing to update is a knob that does nothing"
    );
}

#[test]
fn test_the_map_source_does_not_disturb_the_rest_of_the_config() {
    let config = render_config(&[]);
    assert_eq!(config["database"]["type"], Value::from("sqlite"));
    assert_eq!(config["log"]["level"], Value::from("info"));
    assert_eq!(config["logtail"]["enabled"], Value::from(false));
}
