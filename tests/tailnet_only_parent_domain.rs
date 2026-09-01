//! A Tailnet-only App's parent domain is the App's, not the fleet's.
//!
//! DNS Publication composes a Tailnet-only App's FQDN as
//! `<subdomain>.<parent domain>`, and both halves of that were once one
//! answer: `blocky`'s `customDNS` map interpolated the fleet's `domain` for
//! every entry, and the deploy-time check resolved the same key once per run.
//! ADR-0068 gave the agent tier its own Cloudflare zone, so `essaim` composes
//! against `agents_domain` — a second answer the composition had no way to
//! state.
//!
//! The declaration is the Meta's `domain_key:`, the *Key Registry key* holding
//! the domain rather than the domain itself, so a Host serving a second zone
//! answers for itself under `[hosts.<name>]` (ADR-0058) and no domain is
//! written into the repo.
//!
//! Three properties:
//!
//! - an App declaring `domain_key` publishes under the domain that key
//!   answers;
//! - an App declaring none publishes under `domain`, which is every App in the
//!   fleet but one, so the rendered map does not move;
//! - an App whose key is unanswered publishes **nothing**. An operator who
//!   never onboarded the second zone would otherwise get `essaim.` in Blocky's
//!   map — a name that is not a name, in a config blocky then refuses to load,
//!   taking every other Tailnet-only App's resolution with it.
//!
//! ## The role's own expression is evaluated, not matched
//!
//! A text scan over the `set_fact` cannot tell reading the key from reading it
//! and throwing the answer away — the mutation that defeated #755's first
//! fence. So the task's `vars:` chain is evaluated here, in dependency order,
//! against a written-out context, and the FQDN it produces is compared to a
//! literal. `when:` is evaluated the same way, because "publishes nothing" is a
//! claim about the guard and not about the expression.
//!
//! Two names are seeded rather than evaluated, both of them derivations of the
//! loop item that `blocky_tailnet_addresses.rs` already answers for and this
//! file does not ask about: `parsed`, the Meta the scan slurped, and
//! `app_name`, the App its filename names.

use std::collections::BTreeMap;

use minijinja::value::{Kwargs, Value as JValue};
use minijinja::{Environment, State, UndefinedBehavior};
use serde_yaml::Value;

mod common;

use auberge::playbook_meta::{DEFAULT_DOMAIN_KEY, PlaybookMeta};
use common::{Task, field, meta_files, parse_yaml, registry_keys, relative, role_tasks};

/// The Meta field naming the key an App's FQDN composes against. One spelling,
/// read by two consumers in two languages: the assertions below hold the
/// ansible half to it by evaluating the expression that reads it, and
/// [`test_the_crate_reads_the_field_the_role_reads`] holds the Rust half.
const DOMAIN_KEY_FIELD: &str = "domain_key";

/// The names this harness supplies instead of deriving. See the module docs.
const SEEDED: &[&str] = &["parsed", "app_name"];

/// The fleet's domain, and the agent tier's. Both written out, so an edit that
/// moves which key an entry composes against has to move an assertion too
/// (ADR-0046).
const FLEET_DOMAIN: &str = "example.com";
const AGENTS_DOMAIN: &str = "agents-example.com";

/// Render the way ansible's own jinja will: `trim_blocks` on, and undefined
/// strict — minijinja renders an unknown name as empty, which would turn every
/// FQDN below into a bare subdomain and leave the assertions passing over a
/// composition that reads nothing.
fn jinja() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    // `lookup('vars', name, default=…)`: ansible's own indirect read, and the
    // only way an expression can name a variable it computed. The default is
    // what makes an unset key answerable rather than fatal, so it is honoured
    // here exactly as ansible honours it.
    env.add_function(
        "lookup",
        |state: &State, plugin: String, name: String, kwargs: Kwargs| {
            assert_eq!(plugin, "vars", "the role looks up variables, not files");
            let fallback: Option<JValue> = kwargs.get("default").ok();
            kwargs.assert_all_used()?;
            Ok::<JValue, minijinja::Error>(
                state
                    .lookup(&name)
                    .filter(|value| !value.is_undefined())
                    .or(fallback)
                    .unwrap_or(JValue::UNDEFINED),
            )
        },
    );
    env
}

/// The task that accumulates the map, the one whose `vars:` chain composes the
/// FQDN. Absent is a hard stop: every assertion below is about this task.
fn accumulator() -> Task {
    let mut found: Vec<Task> = role_tasks("blocky")
        .into_iter()
        .filter(|task| {
            field(&task.body, "ansible.builtin.set_fact")
                .and_then(Value::as_mapping)
                .is_some_and(|facts| {
                    facts.contains_key(Value::from("blocky_tailscale_domain_addresses"))
                })
                && field(&task.body, "loop").is_some()
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one task must accumulate the tailnet-only map over the meta scan"
    );
    found.remove(0)
}

/// One task-local `vars:` entry, name to expression, in declaration order.
fn task_vars(task: &Task) -> Vec<(String, String)> {
    let vars = field(&task.body, "vars")
        .and_then(Value::as_mapping)
        .expect("the accumulator composes the FQDN in a `vars:` block");
    vars.iter()
        .filter_map(|(name, expr)| Some((name.as_str()?.to_string(), expr.as_str()?.to_string())))
        .collect()
}

/// The accumulator's `vars:` chain, evaluated against `context` in dependency
/// order — an entry that reads a name not yet bound is retried once the entry
/// binding it has run. A chain that never converges is a hard stop rather than
/// a partial context, since a missing binding reads as an empty FQDN.
fn evaluate(context: &BTreeMap<String, JValue>) -> BTreeMap<String, JValue> {
    let env = jinja();
    let task = accumulator();
    let mut bound = context.clone();
    let mut pending: Vec<(String, String)> = task_vars(&task)
        .into_iter()
        .filter(|(name, _)| !SEEDED.contains(&name.as_str()))
        .collect();

    while !pending.is_empty() {
        let before = pending.len();
        let mut blocked: Vec<String> = Vec::new();
        pending.retain(|(name, expression)| {
            match env.render_str(expression, JValue::from_serialize(&bound)) {
                Ok(rendered) => {
                    bound.insert(name.clone(), JValue::from(rendered));
                    false
                }
                Err(e) => {
                    blocked.push(format!("{name}: {e}"));
                    true
                }
            }
        });
        assert!(
            pending.len() < before,
            "the accumulator's `vars:` chain does not resolve; stuck on {blocked:?}"
        );
    }
    bound
}

/// Whether every `when:` clause standing over the accumulator holds — what
/// decides that an entry reaches the map at all.
fn guards_hold(bound: &BTreeMap<String, JValue>) -> bool {
    let env = jinja();
    let task = accumulator();
    assert!(
        !task.guards.is_empty(),
        "the accumulator must be guarded; an unguarded one maps every Meta in the tree"
    );
    task.guards.iter().all(|clause| {
        let rendered = env
            .render_str(
                &format!("{{{{ ({clause}) | bool }}}}"),
                JValue::from_serialize(bound),
            )
            .unwrap_or_else(|e| panic!("`when: {clause}` must evaluate: {e}"));
        rendered == "true"
    })
}

/// A Meta as the scan hands it over, plus the variables in scope on the Host
/// the infrastructure play targets.
fn context(meta: &str, app: &str, vars: &[(&str, &str)]) -> BTreeMap<String, JValue> {
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(meta).expect("the fixture Meta must parse as YAML");
    let mut context: BTreeMap<String, JValue> = BTreeMap::new();
    context.insert("parsed".to_string(), JValue::from_serialize(&parsed));
    context.insert("app_name".to_string(), JValue::from(app));
    for (name, value) in vars {
        context.insert((*name).to_string(), JValue::from(*value));
    }
    context
}

/// The FQDN and address one Meta lands in the map, or `None` where the guards
/// keep it out.
fn published(meta: &str, app: &str, vars: &[(&str, &str)]) -> Option<(String, String)> {
    let bound = evaluate(&context(meta, app, vars));
    if !guards_hold(&bound) {
        return None;
    }
    let read = |name: &str| {
        bound
            .get(name)
            .unwrap_or_else(|| panic!("the accumulator must bind `{name}`"))
            .to_string()
    };
    Some((read("app_fqdn"), read("app_address")))
}

const AGENT_TIER_META: &str = "subdomain: essaim\ndomain_key: agents_domain\ntailnet_only: true\n";
const FLEET_META: &str = "subdomain: docs\ntailnet_only: true\n";

// ── What the composition publishes ────────────────────────────────────────

#[test]
fn test_an_app_declaring_a_domain_key_publishes_under_that_domain() {
    assert_eq!(
        published(
            AGENT_TIER_META,
            "aoe",
            &[
                ("domain", FLEET_DOMAIN),
                ("agents_domain", AGENTS_DOMAIN),
                ("aoe_tailscale_ip", "100.64.0.2"),
            ],
        ),
        Some((
            "essaim.agents-example.com".to_string(),
            "100.64.0.2".to_string()
        )),
        "the agent tier holds its own zone (ADR-0068), so `essaim` composes against it"
    );
}

#[test]
fn test_an_app_declaring_none_publishes_under_the_fleet_domain() {
    assert_eq!(
        published(
            FLEET_META,
            "paperless",
            &[("domain", FLEET_DOMAIN), ("agents_domain", AGENTS_DOMAIN)],
        ),
        Some(("docs.example.com".to_string(), String::new())),
        "every App in the fleet but one declares no key and must not move"
    );
}

/// The operator override still wins over the Meta's default, on both halves of
/// the composition: `<app>_subdomain` names the label, `domain_key` names where
/// the label sits.
#[test]
fn test_the_subdomain_override_composes_against_the_declared_domain() {
    assert_eq!(
        published(
            AGENT_TIER_META,
            "aoe",
            &[
                ("domain", FLEET_DOMAIN),
                ("agents_domain", AGENTS_DOMAIN),
                ("aoe_subdomain", "swarm"),
            ],
        )
        .map(|(fqdn, _)| fqdn),
        Some("swarm.agents-example.com".to_string()),
    );
}

/// The property the other two cannot state. An unanswered key is the state of
/// every operator who has not onboarded a second zone, and the entry it would
/// otherwise produce is `essaim.` — which blocky refuses to load, taking the
/// resolution of every other Tailnet-only App down with it.
#[test]
fn test_an_app_whose_domain_key_is_unanswered_publishes_nothing() {
    assert_eq!(
        published(AGENT_TIER_META, "aoe", &[("domain", FLEET_DOMAIN)]),
        None,
        "an App whose parent domain has no answer must be left out of the map"
    );
}

/// The same guard, read from the other side: the fleet's own Apps are not
/// collateral of that exclusion.
#[test]
fn test_the_fleet_still_publishes_when_the_second_zone_is_unset() {
    assert!(
        published(FLEET_META, "paperless", &[("domain", FLEET_DOMAIN)]).is_some(),
        "an operator with no second zone must still publish the Apps that use the first"
    );
}

/// A Meta that is not Tailnet-only reaches no entry whatever it declares —
/// unchanged, and worth stating beside a new field that could look like a
/// second selector.
#[test]
fn test_a_public_app_reaches_no_entry() {
    assert_eq!(
        published(
            "subdomain: share\ndomain_key: agents_domain\n",
            "gokapi",
            &[("domain", FLEET_DOMAIN), ("agents_domain", AGENTS_DOMAIN)],
        ),
        None,
        "Blocky's map is the Tailnet-only channel (ADR-0003)"
    );
}

// ── What the tree may declare ─────────────────────────────────────────────

/// Every Meta's `domain_key`, where it declares one.
fn declared_domain_keys() -> Vec<(String, String, bool)> {
    meta_files()
        .into_iter()
        .filter_map(|(app, path)| {
            let meta = parse_yaml(&path);
            let key = meta[DOMAIN_KEY_FIELD].as_str()?.to_string();
            assert!(
                !key.trim().is_empty(),
                "{}: a blank `{DOMAIN_KEY_FIELD}` names no key",
                relative(&path)
            );
            Some((app, key, meta["tailnet_only"].as_bool().unwrap_or(false)))
        })
        .collect()
}

/// A declared key the Key Registry does not hold can never be answered, so the
/// App publishes nothing and the deploy reports success for it.
#[test]
fn test_every_declared_domain_key_is_a_registry_key() {
    let registry = registry_keys();
    let unknown: Vec<String> = declared_domain_keys()
        .into_iter()
        .filter(|(_, key, _)| !registry.contains(key))
        .map(|(app, key, _)| format!("{app}: {key}"))
        .collect();
    assert!(
        unknown.is_empty(),
        "a `{DOMAIN_KEY_FIELD}` outside ansible/keys.yml is a key no config can set, \
         so the App is silently dropped from Blocky's map: {unknown:?}"
    );
}

/// The field's reach is the Tailnet-only channel. Cloudflare publication still
/// composes one zone per run — `plan_set_all` takes a single `domain` — so a
/// Public App in a second zone would publish an A record in the wrong zone
/// rather than fail. It fails here instead.
#[test]
fn test_only_a_tailnet_only_app_declares_a_domain_key() {
    let public: Vec<String> = declared_domain_keys()
        .into_iter()
        .filter(|(_, _, tailnet_only)| !tailnet_only)
        .map(|(app, key, _)| format!("{app}: {key}"))
        .collect();
    assert!(
        public.is_empty(),
        "`{DOMAIN_KEY_FIELD}` is read by Blocky's map and the deploy-time check, neither \
         of which a Public App uses; Cloudflare publication is still one zone per run: {public:?}"
    );
}

// ── The two consumers read one field ──────────────────────────────────────

/// The crate deserializes the spelling the role reads, and defaults it to the
/// key the role defaults it to. Renaming one side alone would leave the map
/// composing against `domain` while the deploy check verified `agents_domain`,
/// and neither would say so.
#[test]
fn test_the_crate_reads_the_field_the_role_reads() {
    let declared: PlaybookMeta = serde_yaml::from_str(&format!(
        "required_keys: []\n{DOMAIN_KEY_FIELD}: agents_domain\n"
    ))
    .expect("a Meta naming a parent domain key must parse");
    assert_eq!(declared.parent_domain_key(), "agents_domain");

    let bare: PlaybookMeta =
        serde_yaml::from_str("required_keys: []\n").expect("a Meta naming none must parse");
    assert_eq!(bare.parent_domain_key(), DEFAULT_DOMAIN_KEY);

    let expression = task_vars(&accumulator())
        .into_iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        expression.contains(DOMAIN_KEY_FIELD),
        "the accumulator's `vars:` must read `{DOMAIN_KEY_FIELD}` off the Meta"
    );
    assert!(
        expression.contains(&format!("'{DEFAULT_DOMAIN_KEY}'")),
        "the accumulator must default to `{DEFAULT_DOMAIN_KEY}` for an App declaring no key"
    );
}
