//! The tailnet's global resolver is the Host's own Blocky.
//!
//! Tailscale SaaS pushed Blocky — the Host's tailnet IP — as the tailnet's
//! *global* resolver, so every query from every device was filtered.
//! `headscale-config.yaml.j2` hardcoded `1.1.1.1`/`1.0.0.1` there and put
//! Blocky behind a `split` entry for `{{ domain }}` only, which is the shape
//! ADR-0003 chose and ADR-0052 reverses. Migrating off SaaS with the old
//! template would have silently unfiltered every client's general browsing —
//! silently, because ad-blocking that stops working looks exactly like a
//! tailnet that works (#708).
//!
//! Two properties, and the second is the one a reader would not think to keep:
//!
//! - the rendered `global` list is the discovered IP when there is one, and the
//!   public pair only while the Host has none;
//! - the discovery runs on *every* deploy. It used to run only when the
//!   operator had left `headscale_split_dns_target_ip` empty, because deriving
//!   that target was the only thing it fed. Leaving that guard in place while
//!   adding a second consumer means an operator who sets a split target by hand
//!   also turns off the global filter, which is not a trade anyone would make on
//!   purpose.
//!
//! The fallback carries a third. `tailscale status` answers "this Host has no
//! tailnet IP" and "I could not tell you" with the same silence, and only the
//! first is a reason to render a public resolver. On a Host that already serves
//! a tailnet resolver, an unreachable `tailscaled` would otherwise re-render the
//! public pair over it and unfilter every enrolled client — the very failure
//! this ADR exists to prevent, arriving through the repair path instead of the
//! migration. So the role reads the config it already deployed and refuses.

use std::collections::HashMap;
use std::fs;

use serde_yaml::Value;

mod common;

use common::{Plays, Task, field, role_dir, strings, task_name, tasks_in};

/// The public resolvers the config falls back to before the Host holds a
/// tailnet IP of its own — the first headscale deploy precedes the Host's
/// enrolment, the ordering ADR-0003 already names.
const FALLBACK_NAMESERVERS: [&str; 2] = ["1.1.1.1", "1.0.0.1"];

const DISCOVERED_FACT: &str = "headscale_tailscale_ipv4";
const SPLIT_TARGET: &str = "headscale_split_dns_target_ip";
const FALLBACK_LIST: &str = "headscale_fallback_nameservers";
const DEPLOYED_NAMESERVER: &str = "headscale_deployed_global_nameserver";

/// A list the role declares, so the fence renders what a deploy renders rather
/// than a second copy that can drift from it.
fn declared_list(key: &str) -> Vec<String> {
    let defaults = fs::read_to_string(role_dir("headscale").join("defaults/main.yml"))
        .expect("headscale defaults must exist");
    let parsed: Value = serde_yaml::from_str(&defaults).expect("headscale defaults must parse");
    strings(Some(&parsed[key]))
}

fn declared_fallback_nameservers() -> Vec<String> {
    declared_list(FALLBACK_LIST)
}

/// Render `headscale-config.yaml.j2` the way ansible will. `trim_blocks`
/// defaults to true there and to false in jinja2 itself, so a fence that leaves
/// it at minijinja's default renders a document ansible never produces.
fn render_config(discovered_ipv4: Option<&str>, split_target: &str) -> String {
    let template =
        fs::read_to_string(role_dir("headscale").join("templates/headscale-config.yaml.j2"))
            .expect("headscale-config.yaml.j2 must exist");

    let mut ctx: HashMap<&str, minijinja::Value> = HashMap::new();
    for (key, value) in [
        ("headscale_server_url", "https://hs.example.com"),
        ("headscale_port", "8080"),
        ("headscale_metrics_port", "9091"),
        ("headscale_stun_port", "3478"),
        ("headscale_data_dir", "/var/lib/headscale"),
        ("headscale_db_path", "/var/lib/headscale/db.sqlite"),
        ("headscale_ip_prefix_v4", "100.64.0.0/10"),
        ("headscale_ip_prefix_v6", "fd7a:115c:a1e0::/48"),
        ("headscale_derp_enabled", "true"),
        ("headscale_derp_region_id", "999"),
        ("headscale_derp_region_name", "self-hosted"),
        ("headscale_derp_update_frequency", "24h"),
        ("headscale_magic_dns_enabled", "true"),
        ("headscale_base_domain", "ts.example.com"),
        ("headscale_log_level", "info"),
        ("domain", "example.com"),
        (SPLIT_TARGET, split_target),
    ] {
        ctx.insert(key, minijinja::Value::from(value));
    }
    for key in [FALLBACK_LIST, "headscale_derp_urls"] {
        ctx.insert(key, minijinja::Value::from(declared_list(key)));
    }
    // Absent, not empty, when nothing was discovered: that is the shape the
    // role leaves behind when its `set_fact` is skipped.
    if let Some(ipv4) = discovered_ipv4 {
        ctx.insert(DISCOVERED_FACT, minijinja::Value::from(ipv4));
    }

    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("headscale-config.yaml.j2 must render")
}

/// The rendered document, parsed. Every assertion reads the config headscale
/// reads rather than a line of text, so a change that keeps the substring and
/// breaks the document fails here.
fn rendered_dns(discovered_ipv4: Option<&str>, split_target: &str) -> Value {
    let rendered = render_config(discovered_ipv4, split_target);
    let parsed: Value = serde_yaml::from_str(&rendered)
        .unwrap_or_else(|e| panic!("rendered config must parse: {e}\n{rendered}"));
    parsed["dns"].clone()
}

/// `common::strings` reads a missing key as `[]`, so every assertion below
/// would pass on a config that lost its `global` list entirely. The domain is
/// asserted here instead, once, rather than in each caller.
fn global_nameservers(discovered_ipv4: Option<&str>, split_target: &str) -> Vec<String> {
    let global = rendered_dns(discovered_ipv4, split_target)["nameservers"]["global"].clone();
    assert!(
        global.is_sequence(),
        "`dns.nameservers.global` must be a list, not {global:?}"
    );
    let nameservers = strings(Some(&global));
    assert!(
        !nameservers.is_empty(),
        "a tailnet with no global nameserver resolves nothing"
    );
    nameservers
}

#[test]
fn test_global_nameserver_is_the_hosts_blocky() {
    assert_eq!(
        global_nameservers(Some("100.64.0.1"), ""),
        vec!["100.64.0.1"],
        "an enrolled Host filters every tailnet query through its own Blocky"
    );
}

#[test]
fn test_blocky_is_the_only_global_nameserver() {
    let global = global_nameservers(Some("100.64.0.1"), "");
    for fallback in FALLBACK_NAMESERVERS {
        assert!(
            !global.contains(&fallback.to_string()),
            "a public resolver beside Blocky is an unfiltered path off the filter: {global:?}"
        );
    }
}

#[test]
fn test_global_falls_back_to_public_resolvers_before_the_host_enrols() {
    assert_eq!(
        global_nameservers(None, ""),
        FALLBACK_NAMESERVERS,
        "the first headscale deploy runs before the Host can have a tailnet IP"
    );
}

/// The role cannot produce an empty `headscale_tailscale_ipv4` — its `first`
/// raises on an empty match rather than yielding `""` — so this fences the
/// template's own guard, not a state the role reaches. Kept because the guard
/// is the one `blocky/templates/config.yaml.j2` carries, and a template that
/// renders `- ` as a nameserver is worth failing on wherever the value came
/// from.
#[test]
fn test_the_template_never_renders_an_empty_nameserver() {
    assert_eq!(
        global_nameservers(Some(""), ""),
        FALLBACK_NAMESERVERS,
        "an empty value must read as absent, never render as a resolver"
    );
}

#[test]
fn test_split_dns_stays_the_operators_own_mechanism() {
    let dns = rendered_dns(Some("100.64.0.1"), "203.0.113.9");
    assert_eq!(
        dns["nameservers"]["split"]["example.com"],
        Value::Sequence(vec![Value::from("203.0.113.9")]),
        "an operator-set split target renders whatever the global nameserver is"
    );
}

#[test]
fn test_no_split_block_without_a_target() {
    for discovered in [None, Some("100.64.0.1")] {
        let dns = rendered_dns(discovered, "");
        assert!(
            dns["nameservers"]["split"].is_null(),
            "an empty split target must emit no split block (discovered={discovered:?})"
        );
    }
}

#[test]
fn test_the_directive_after_the_nameservers_survives_ansible_trim_blocks() {
    for discovered in [None, Some("100.64.0.1"), Some("")] {
        for split in ["", "203.0.113.9"] {
            let dns = rendered_dns(discovered, split);
            assert!(
                !dns.is_null(),
                "trim_blocks joined a directive onto the nameserver list \
                 (discovered={discovered:?}, split={split:?})"
            );
        }
    }
}

fn headscale_tasks() -> Vec<Task> {
    tasks_in(
        &role_dir("headscale").join("tasks/main.yml"),
        Plays::AsTasks,
    )
}

/// The task that binds a name, and every `when:` standing over it.
fn task_setting(fact: &str) -> Task {
    headscale_tasks()
        .into_iter()
        .find(|task| {
            field(&task.body, "ansible.builtin.set_fact")
                .and_then(Value::as_mapping)
                .is_some_and(|set| set.contains_key(Value::from(fact)))
        })
        .unwrap_or_else(|| panic!("the headscale role must set `{fact}`"))
}

#[test]
fn test_the_discovered_ip_is_a_fact_of_its_own() {
    let guards = task_setting(DISCOVERED_FACT).guards.join(" ");
    assert!(
        !guards.contains(SPLIT_TARGET),
        "discovery gated on `{SPLIT_TARGET}` stops filtering the tailnet the \
         moment an operator sets a split target by hand: {guards}"
    );
}

#[test]
fn test_the_tailscale_probe_runs_on_every_deploy() {
    let probes: Vec<Task> = headscale_tasks()
        .into_iter()
        .filter(|task| {
            field(&task.body, "ansible.builtin.command")
                .and_then(Value::as_str)
                .is_some_and(|command| command.contains("tailscale status"))
        })
        .collect();

    assert_eq!(
        probes.len(),
        1,
        "one probe feeds both consumers; a second one is a second answer"
    );
    let probe = &probes[0];
    assert!(
        !probe.guards.join(" ").contains(SPLIT_TARGET),
        "`{}` must run unconditionally: the global nameserver needs its answer \
         whatever the operator did with `{SPLIT_TARGET}`",
        task_name(&probe.body)
    );
}

#[test]
fn test_the_split_target_derives_from_the_same_discovery() {
    let derive = task_setting(SPLIT_TARGET);
    let expression = field(&derive.body, "ansible.builtin.set_fact")
        .and_then(Value::as_mapping)
        .and_then(|set| set.get(Value::from(SPLIT_TARGET)))
        .and_then(Value::as_str)
        .expect("the split target must derive from an expression")
        .to_string();
    assert!(
        expression.contains(DISCOVERED_FACT),
        "two parses of the same `tailscale status` output are two chances to \
         disagree; the split target reads the fact: {expression}"
    );
    assert!(
        derive.guards.join(" ").contains(SPLIT_TARGET),
        "an operator-set split target must still win over the derived one"
    );
}

#[test]
fn test_the_fallback_list_is_declared_in_role_defaults() {
    assert_eq!(
        declared_fallback_nameservers(),
        FALLBACK_NAMESERVERS,
        "the public pair is a role default, not a literal in the template"
    );
}

/// `tailscale status` says "no tailnet IP here" and "I cannot answer" the same
/// way. Only the first is a reason to render a public resolver, and the config
/// already on disk is what tells them apart: a Host serving a tailnet resolver
/// was enrolled, so the answer is missing rather than absent.
#[test]
fn test_a_host_already_serving_a_tailnet_resolver_refuses_the_public_fallback() {
    let refusal = headscale_tasks()
        .into_iter()
        .find(|task| field(&task.body, "ansible.builtin.fail").is_some())
        .expect("the role must refuse to downgrade a tailnet resolver");

    let guards = refusal.guards.join(" ");
    for required in [
        format!("{DISCOVERED_FACT} is not defined"),
        format!("{DEPLOYED_NAMESERVER} is defined"),
        format!("{DEPLOYED_NAMESERVER} not in {FALLBACK_LIST}"),
    ] {
        assert!(
            guards.contains(&required),
            "the refusal must fire on exactly `{required}`, not on {guards}"
        );
    }
}

#[test]
fn test_the_deployed_nameserver_is_read_from_the_deployed_config() {
    let expression = field(
        &task_setting(DEPLOYED_NAMESERVER).body,
        "ansible.builtin.set_fact",
    )
    .and_then(Value::as_mapping)
    .and_then(|set| set.get(Value::from(DEPLOYED_NAMESERVER)))
    .and_then(Value::as_str)
    .expect("the deployed nameserver must derive from an expression")
    .to_string();
    assert!(
        expression.contains("dns.nameservers.global"),
        "a note the role wrote is not evidence; read the config it deployed: {expression}"
    );
}
