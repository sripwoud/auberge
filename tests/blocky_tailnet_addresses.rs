//! A Tailnet-only App's address is the App's, not the Blocky Host's.
//!
//! DNS Publication for a Tailnet-only App (ADR-0003) runs entirely through
//! Blocky's `customDNS` map, and that map used to interpolate one address for
//! every entry in it — `blocky_tailscale_ipv4`, read off `tailscale status` on
//! the Host running Blocky. The map was built by a `run_once` scan that already
//! reaches every `*.meta.yml` in the fleet, so a Tailnet-only App on a second
//! Host was discovered and then published at the wrong address (#755).
//!
//! Three properties, and the third is the one that makes the other two safe to
//! ship:
//!
//! - an App declaring `<app>_tailscale_ip` publishes that address;
//! - an App declaring none publishes the Blocky Host's own tailnet IPv4, which
//!   is every App in the fleet today, so the rendered map does not move;
//! - both hold in *one* rendered config. A per-entry address that renders one
//!   value per config is the same defect with more machinery, and a fence that
//!   renders a single entry at a time cannot tell the two apart.
//!
//! ## Expectations are literals
//!
//! Every address asserted below is written out rather than read back from the
//! context the render was fed, so an edit that moves the value has to move the
//! assertion too (ADR-0046). The two fixture addresses are deliberately both
//! inside `100.64.0.0/10`: a fence that could only tell them apart because one
//! was not a tailnet address would pass over a template that special-cased the
//! range instead of reading the entry.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use minijinja::{Environment, UndefinedBehavior};
use serde_yaml::Value;

mod common;

use common::{Task, field, meta_files, parse_yaml, role_dir, role_tasks, strings, task_name};

/// The Blocky Host's own tailnet IPv4 — what an App declaring no address of its
/// own falls back to.
const BLOCKY_HOST: &str = "100.64.0.1";

/// A second Host's tailnet IPv4. In `100.64.0.0/10` like [`BLOCKY_HOST`], so
/// nothing but the entry it belongs to distinguishes them.
const OTHER_HOST: &str = "100.64.0.2";

/// The fact the role builds and the template reads. Named once so
/// [`test_the_template_reads_the_fact_the_role_builds`] can hold the two ends
/// of it together.
const ADDRESS_MAP: &str = "blocky_tailscale_domain_addresses";

/// The Blocky Host's discovered address, set by the role's own `tailscale
/// status` probe.
const BLOCKY_IPV4: &str = "blocky_tailscale_ipv4";

/// What the fleet-wide meta scan registers. The blocky role runs a second,
/// unrelated `find` over the Lego account layout, so the register name is what
/// separates the scan this fence is about from that one.
const META_SCAN_REGISTER: &str = "blocky_meta_files_found";

/// The per-App config key. The Public-App half of `discover_all_subdomains`
/// reads the same one, which is the whole point of the shape: an App's tailnet
/// address means one thing across both publication channels.
const ADDRESS_KEY_SUFFIX: &str = "_tailscale_ip";

/// Render the way `ansible.builtin.template` will.
///
/// `trim_blocks` is on because ansible defaults it on where jinja2's own
/// default is off. Undefined is strict because minijinja's default renders an
/// unknown name as empty, which here would silently produce `fqdn:` with no
/// address and leave every assertion below passing over a config blocky cannot
/// load.
fn jinja() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env
}

fn template() -> String {
    fs::read_to_string(role_dir("blocky").join("templates/config.yaml.j2"))
        .expect("blocky config.yaml.j2 must exist")
}

/// `config.yaml.j2`, rendered against `addresses` (FQDN to the address its App
/// declares, empty for an App declaring none) and the Blocky Host's own IPv4.
fn render(addresses: &[(&str, &str)], blocky_ipv4: Option<&str>) -> String {
    let mut ctx: BTreeMap<&str, minijinja::Value> = BTreeMap::new();
    for (key, value) in [
        ("blocky_cert_file", "/etc/blocky/certificates/dns.crt"),
        ("blocky_key_file", "/etc/blocky/certificates/dns.key"),
        ("blocky_dot_port", "853"),
    ] {
        ctx.insert(key, minijinja::Value::from(value));
    }
    let map: BTreeMap<String, String> = addresses
        .iter()
        .map(|(fqdn, address)| ((*fqdn).to_string(), (*address).to_string()))
        .collect();
    ctx.insert(ADDRESS_MAP, minijinja::Value::from_serialize(&map));
    // Absent, not empty, when the probe found nothing: that is the shape the
    // role leaves behind when its `set_fact` is skipped.
    if let Some(ipv4) = blocky_ipv4 {
        ctx.insert(BLOCKY_IPV4, minijinja::Value::from(ipv4));
    }

    jinja()
        .render_str(&template(), ctx)
        .expect("blocky config.yaml.j2 must render")
}

/// The rendered `customDNS.mapping`, read out of the parsed document rather
/// than off a line of text, so a change that keeps the substring and breaks the
/// YAML fails here.
fn mapping(addresses: &[(&str, &str)], blocky_ipv4: Option<&str>) -> BTreeMap<String, String> {
    let rendered = render(addresses, blocky_ipv4);
    let parsed: Value = serde_yaml::from_str(&rendered)
        .unwrap_or_else(|e| panic!("rendered config must parse: {e}\n{rendered}"));
    let custom = &parsed["customDNS"]["mapping"];
    if custom.is_null() {
        return BTreeMap::new();
    }
    let entries = custom
        .as_mapping()
        .unwrap_or_else(|| panic!("`customDNS.mapping` must be a mapping, not {custom:?}"));
    entries
        .iter()
        .map(|(fqdn, address)| {
            let fqdn = fqdn.as_str().expect("an FQDN must render as a string");
            let address = address
                .as_str()
                .unwrap_or_else(|| panic!("{fqdn} must render an address, not {address:?}"));
            (fqdn.to_string(), address.to_string())
        })
        .collect()
}

// ── The rendered map ──────────────────────────────────────────────────────

#[test]
fn test_an_app_declaring_an_address_publishes_it() {
    let published = mapping(&[("ruche.example.com", OTHER_HOST)], Some(BLOCKY_HOST));
    assert_eq!(
        published.get("ruche.example.com").map(String::as_str),
        Some(OTHER_HOST),
        "a Tailnet-only App on another Host resolves to that Host"
    );
}

#[test]
fn test_an_app_declaring_none_falls_back_to_the_blocky_host() {
    let published = mapping(&[("paperless.example.com", "")], Some(BLOCKY_HOST));
    assert_eq!(
        published.get("paperless.example.com").map(String::as_str),
        Some(BLOCKY_HOST),
        "every App in the fleet today declares no address and must not move"
    );
}

/// The property the other two cannot state on their own: the address is per
/// entry, not per rendered config.
#[test]
fn test_two_apps_get_two_addresses_in_one_config() {
    let published = mapping(
        &[
            ("paperless.example.com", ""),
            ("ruche.example.com", OTHER_HOST),
        ],
        Some(BLOCKY_HOST),
    );
    assert_eq!(
        published,
        BTreeMap::from([
            ("paperless.example.com".to_string(), BLOCKY_HOST.to_string()),
            ("ruche.example.com".to_string(), OTHER_HOST.to_string()),
        ]),
        "one config publishes each App at its own Host"
    );
}

/// Blocky answers on the tailnet or it answers nowhere, so a Host with no
/// tailnet address of its own publishes no map at all — including for an App
/// that declares an address, which nothing could reach through a resolver that
/// is not listening on the tailnet.
#[test]
fn test_no_mapping_before_the_blocky_host_holds_a_tailnet_ip() {
    for addresses in [
        &[("paperless.example.com", "")][..],
        &[("ruche.example.com", OTHER_HOST)][..],
    ] {
        let rendered = render(addresses, None);
        let parsed: Value = serde_yaml::from_str(&rendered)
            .unwrap_or_else(|e| panic!("rendered config must parse: {e}\n{rendered}"));
        assert!(
            parsed["customDNS"].is_null(),
            "a Host off the tailnet must publish no customDNS block: {rendered}"
        );
    }
}

#[test]
fn test_no_mapping_without_a_tailnet_only_app() {
    assert!(
        mapping(&[], Some(BLOCKY_HOST)).is_empty(),
        "an empty map must emit no customDNS block"
    );
}

/// Every Tailnet-only App the repo declares today, and the subdomain its Meta
/// carries. Read off the live tree so a new one is covered by existing.
fn tailnet_only_apps() -> Vec<(String, String)> {
    let apps: Vec<(String, String)> = meta_files()
        .into_iter()
        .filter_map(|(app, path)| {
            let meta = parse_yaml(&path);
            meta["tailnet_only"].as_bool().unwrap_or(false).then(|| {
                let subdomain = meta["subdomain"]
                    .as_str()
                    .unwrap_or_else(|| {
                        panic!("{app}: a tailnet-only Meta must declare a subdomain")
                    })
                    .to_string();
                (app, subdomain)
            })
        })
        .collect();
    assert!(
        !apps.is_empty(),
        "the tree declares no Tailnet-only App, so this fence reasons about nothing"
    );
    apps
}

/// The fleet does not move. No App sets `<app>_tailscale_ip` today, so every
/// Tailnet-only App in the tree still resolves to the Blocky Host — asserted
/// over the Apps that exist rather than over invented ones, since "unchanged
/// for the current fleet" is a claim about them.
#[test]
fn test_every_tailnet_only_app_in_the_tree_falls_back_to_the_blocky_host() {
    let apps = tailnet_only_apps();
    let fqdns: Vec<String> = apps
        .iter()
        .map(|(_, subdomain)| format!("{subdomain}.example.com"))
        .collect();
    let addresses: Vec<(&str, &str)> = fqdns.iter().map(|fqdn| (fqdn.as_str(), "")).collect();

    let published = mapping(&addresses, Some(BLOCKY_HOST));
    for ((app, _), fqdn) in apps.iter().zip(&fqdns) {
        assert_eq!(
            published.get(fqdn).map(String::as_str),
            Some(BLOCKY_HOST),
            "{app} declares no address, so it must still resolve to the Blocky Host"
        );
    }
}

// ── The fact behind it ────────────────────────────────────────────────────

/// Every task in the blocky role that writes `ADDRESS_MAP`.
fn tasks_setting_the_map() -> Vec<Task> {
    role_tasks("blocky")
        .into_iter()
        .filter(|task| {
            field(&task.body, "ansible.builtin.set_fact")
                .and_then(Value::as_mapping)
                .is_some_and(|facts| facts.contains_key(Value::from(ADDRESS_MAP)))
        })
        .collect()
}

/// The template and the role name the same fact. Renaming one and not the
/// other renders a config with no `customDNS` block at all, which is a DNS
/// outage for every Tailnet-only App and a deploy that reports success.
#[test]
fn test_the_template_reads_the_fact_the_role_builds() {
    assert!(
        template().contains(ADDRESS_MAP),
        "config.yaml.j2 must read `{ADDRESS_MAP}`"
    );
    assert_eq!(
        tasks_setting_the_map().len(),
        2,
        "`{ADDRESS_MAP}` is initialized once and accumulated once"
    );
}

/// The template's guard reads the fact's `length`, which raises on an undefined
/// name under ansible's own jinja. The default is what makes the guard
/// answerable on a run whose scan matched nothing.
#[test]
fn test_the_fact_defaults_to_a_mapping() {
    let defaults = fs::read_to_string(role_dir("blocky").join("defaults/main.yml"))
        .expect("blocky defaults must exist");
    let parsed: Value = serde_yaml::from_str(&defaults).expect("blocky defaults must parse");
    assert!(
        parsed[ADDRESS_MAP].is_mapping(),
        "`{ADDRESS_MAP}` must default to a mapping, not {:?}",
        parsed[ADDRESS_MAP]
    );
}

/// One task-local `vars:` entry, name to expression.
fn task_vars(task: &Task) -> BTreeMap<String, String> {
    field(&task.body, "vars")
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

/// The expression that builds the map, with every task-local `vars:` entry it
/// reaches inlined, transitively.
///
/// Ansible resolves `vars:` lazily at use, so a lookup two hops from the
/// `set_fact` is still a lookup the expression performs. Asserting over the
/// `vars:` block alone is what this replaces, and it does not hold: a task that
/// declares `app_address` and then writes `combine({app_fqdn: ''})` reads the
/// key, throws the answer away, and publishes the Blocky Host's address for
/// every App — the exact defect #755 closes — while the key's name still sits
/// in the file for a scan to find. That mutation passed.
fn map_expression_with_vars_inlined(task: &Task) -> String {
    let vars = task_vars(task);
    let mut text = field(&task.body, "ansible.builtin.set_fact")
        .and_then(Value::as_mapping)
        .and_then(|facts| facts.get(Value::from(ADDRESS_MAP)))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the task writing `{ADDRESS_MAP}` must write an expression"))
        .to_string();

    let mut inlined: BTreeSet<String> = BTreeSet::new();
    while let Some((name, expr)) = vars
        .iter()
        .find(|(name, _)| !inlined.contains(*name) && text.contains(name.as_str()))
        .map(|(name, expr)| (name.clone(), expr.clone()))
    {
        inlined.insert(name);
        text.push(' ');
        text.push_str(&expr);
    }
    text
}

/// The address each entry carries comes from the App's own key. Reading it and
/// discarding it leaves every entry empty, which the template then fills with
/// the Blocky Host's address.
#[test]
fn test_the_map_reads_the_apps_own_address_key() {
    let accumulator = tasks_setting_the_map()
        .into_iter()
        .find(|task| field(&task.body, "loop").is_some())
        .expect("one task must accumulate the map over the meta scan");

    assert!(
        map_expression_with_vars_inlined(&accumulator).contains(ADDRESS_KEY_SUFFIX),
        "the expression writing `{ADDRESS_MAP}` never reaches `<app>{ADDRESS_KEY_SUFFIX}`, \
         so every App would publish the Blocky Host's address"
    );
}

/// The scan that feeds the map is fleet-wide and must stay that way. It is
/// `run_once` on the controller over every `*.meta.yml` in the playbooks
/// directory, independent of which playbook deploys the App — which is why a
/// Tailnet-only App on a second Host was already discovered before #755, just
/// mis-addressed. Narrowing it to the Apps this play deploys would delete
/// entries from the map instead of re-addressing them.
#[test]
fn test_the_meta_scan_stays_fleet_wide() {
    let scans: Vec<Task> = role_tasks("blocky")
        .into_iter()
        .filter(|task| {
            field(&task.body, "register").and_then(Value::as_str) == Some(META_SCAN_REGISTER)
        })
        .collect();
    assert_eq!(
        scans.len(),
        1,
        "exactly one task must register `{META_SCAN_REGISTER}`, the scan the map is built from"
    );
    let scan = &scans[0];
    let name = task_name(&scan.body);
    let args = field(&scan.body, "ansible.builtin.find")
        .and_then(Value::as_mapping)
        .unwrap_or_else(|| {
            panic!("{name} must build `{META_SCAN_REGISTER}` with `find`, not from a narrower list")
        });

    assert_eq!(
        strings(args.get(Value::from("paths"))),
        vec!["{{ playbook_dir }}"],
        "{name} must scan the whole playbooks directory"
    );
    assert_eq!(
        strings(args.get(Value::from("patterns"))),
        vec!["*.meta.yml"],
        "{name} must read every App's Meta, not a subset"
    );
    assert_eq!(
        field(&scan.body, "delegate_to").and_then(Value::as_str),
        Some("localhost"),
        "{name} reads the controller's own asset tree"
    );
    assert_eq!(
        field(&scan.body, "run_once").and_then(Value::as_bool),
        Some(true),
        "{name} answers the same question for every Host in the play"
    );
    assert!(
        scan.guards.is_empty(),
        "{name} must run unconditionally: a guard on it narrows the map to nothing, \
         which renders no customDNS block and reports success"
    );
}
