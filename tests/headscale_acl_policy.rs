//! The tailnet runs a tag-based ACL policy — ADR-0055.
//!
//! Since the #712 flag day the tailnet ran headscale's default allow-all: no
//! `policy:` block existed anywhere in the role, and the ACL tags
//! `auberge headscale add-key -t` stamps on pre-auth keys were decorative.
//! ADR-0054 enrolls a node designed on the assumption of compromise (`ruche`),
//! so confinement has to live in the mesh — ufw on the data host cannot tell
//! one tailnet peer from another.
//!
//! The policy is file-backed and repo-owned (the ADR-0051 shape: config on the
//! Host is the single source of truth, not a `headscale policy set` into the
//! DB), so these fences read the file the role ships and the config that points
//! headscale at it, never a note about either.
//!
//! Trust is a tag, and node→tag mapping is a pre-auth-key concern, not a
//! policy-file one — so the policy names tiers, never nodes, and every
//! assertion below is about a tier. Which tier plays which part (resolver,
//! confined) is written out as a literal rather than read back from the file,
//! so widening the policy is a decision made twice (ADR-0046).
//!
//! The tier *vocabulary* is the crate's, reached with `use` (ADR-0046): #767
//! types a Host's trust tier as [`TailnetTag`], and a roster entry is validated
//! against that type rather than against the deployed policy — `TagExists` is a
//! runtime gate on a *loaded* policy (ADR-0061), so it cannot answer for a
//! declaration. This file is what makes the static check honest: the crate's
//! four variants and the shipped policy's `tagOwners` are held equal, in both
//! directions, so neither can widen alone.

use std::fs;
use std::path::PathBuf;

use auberge::hosts::TailnetTag;
use serde_json::Value;

mod common;

use common::{
    Plays, Task, field, relative, role_dir, role_templates, strings, tasks_in, templated_yml_files,
};

/// The trust tiers ADR-0055 declares, and the only tags the policy may name —
/// as the crate spells them, so the policy file is compared against the type
/// `hosts.toml` validates against rather than against a second literal.
fn trust_tiers() -> Vec<String> {
    TailnetTag::ALL.iter().map(|tier| tier.acl_tag()).collect()
}

/// The tier that runs the tailnet's global resolver (ADR-0052): the data
/// host's Blocky. Every node reaches it on 53, and nothing else.
const RESOLVER_TIER: &str = "tag:data";
const RESOLVER_DST: &str = "tag:data:53";

/// The tier enrolled on the assumption of compromise (ADR-0054). It initiates
/// nothing tailnet-side of its own; its only reach is the shared DNS rule.
const CONFINED_TIER: &str = "tag:agent";

fn headscale_role_dir() -> PathBuf {
    role_dir("headscale")
}

fn policy_file() -> PathBuf {
    headscale_role_dir().join("files/policy.hujson")
}

/// HuJSON is JSON with `//` comments and trailing commas. The shipped file uses
/// both and no string value contains `//`, so stripping line comments and
/// trailing commas yields the JSON headscale's parser sees. Kept deliberately
/// small: a `//` inside a future string value would break it, which is a louder
/// failure than a silent misparse.
fn parse_hujson(raw: &str) -> Value {
    let no_comments = raw
        .lines()
        .map(|line| match line.find("//") {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let no_trailing = regex::Regex::new(r",(\s*[}\]])")
        .unwrap()
        .replace_all(&no_comments, "$1");
    serde_json::from_str(&no_trailing)
        .unwrap_or_else(|e| panic!("policy.hujson must parse as HuJSON: {e}\n{no_trailing}"))
}

fn policy() -> Value {
    parse_hujson(&fs::read_to_string(policy_file()).expect("policy.hujson must exist"))
}

fn acls(policy: &Value) -> Vec<Value> {
    policy["acls"]
        .as_array()
        .expect("the policy must carry an acls list")
        .clone()
}

/// A rule's `src` and `dst`, each as a plain list of strings.
fn rule_field(rule: &Value, key: &str) -> Vec<String> {
    rule[key]
        .as_array()
        .unwrap_or_else(|| panic!("every rule must carry a {key} list"))
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("every {key} entry must be a string"))
                .to_string()
        })
        .collect()
}

/// The tier a `dst` entry names, dropping the `:port` suffix — `tag:data:53`
/// and `tag:data:*` both name `tag:data`. A bare `*:*` names the wildcard.
fn dst_tier(entry: &str) -> String {
    match entry.rfind(':') {
        Some(index) => entry[..index].to_string(),
        None => entry.to_string(),
    }
}

fn headscale_tasks() -> Vec<Task> {
    tasks_in(&headscale_role_dir().join("tasks/main.yml"), Plays::AsTasks)
}

/// The `policy:` block carries no `{% %}` conditionals, so the three lines are
/// read from the template text rather than rendering the whole file — a
/// file-mode policy pointed at the same path the role deploys is the ADR-0051
/// shape: config on the Host owns the policy, not a `headscale policy set` into
/// the DB.
#[test]
fn test_the_policy_is_file_backed_and_points_at_the_shipped_file() {
    let path = common::defaults("headscale")
        .get("headscale_policy_path")
        .expect("headscale_policy_path must be a role default")
        .clone();
    assert!(
        path.ends_with("/policy.hujson"),
        "the policy path must name the shipped file, not {path}"
    );

    let template =
        fs::read_to_string(headscale_role_dir().join("templates/headscale-config.yaml.j2"))
            .expect("headscale-config.yaml.j2 must exist");
    let block = template
        .split_once("policy:")
        .map(|(_, rest)| rest)
        .expect("the config must carry a policy: block");
    assert!(
        block.contains("mode: file"),
        "a DB-stored policy is not repo-owned; the file mode is the ADR-0051 shape"
    );
    assert!(
        block.contains("path: {{ headscale_policy_path }}"),
        "config must point headscale at the same file the role deploys, via the shared default"
    );
}

#[test]
fn test_the_role_deploys_the_policy_and_restarts_on_change() {
    let deploy = headscale_tasks()
        .into_iter()
        .find(|task| {
            field(&task.body, "ansible.builtin.copy")
                .and_then(serde_yaml::Value::as_mapping)
                .and_then(|copy| copy.get(serde_yaml::Value::from("src")))
                .and_then(serde_yaml::Value::as_str)
                == Some("policy.hujson")
        })
        .expect("the role must deploy policy.hujson");
    assert!(
        strings(field(&deploy.body, "notify")).contains(&"Restart headscale".to_string()),
        "a policy that changes without restarting headscale never takes effect"
    );
}

#[test]
fn test_the_acls_flip_the_tailnet_to_default_deny() {
    let rules = acls(&policy());
    assert!(
        !rules.is_empty(),
        "an absent or empty acls list is allow-all; the whole point is deny-by-default"
    );
    let allow_all = rules.iter().any(|rule| {
        rule_field(rule, "src").contains(&"*".to_string())
            && rule_field(rule, "dst").contains(&"*:*".to_string())
    });
    assert!(
        !allow_all,
        "a `*` -> `*:*` rule re-opens the tailnet the policy exists to close"
    );
}

#[test]
fn test_the_four_trust_tiers_are_the_only_declared_tags() {
    let owners = policy()["tagOwners"]
        .as_object()
        .expect("the policy must declare tagOwners")
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        owners,
        trust_tiers().into_iter().collect(),
        "the policy's tagOwners and TailnetTag's variants are one vocabulary: a tier in the \
         type but not the policy is a hosts.toml value headscale will refuse, and a tier in the \
         policy but not the type is one no Host can be declared with"
    );
}

/// Empty owner lists mean only an admin may apply the tag. The CLI mints the
/// pre-auth keys as admin, so the policy validates without a baked-in username
/// — a policy that named one operator would break on every other tailnet.
#[test]
fn test_tags_are_admin_owned_not_operator_named() {
    let owners = policy()["tagOwners"].clone();
    for tier in trust_tiers() {
        assert_eq!(
            owners[&tier].as_array().map(Vec::len),
            Some(0),
            "{tier} must be admin-owned (empty list), not owned by a named user"
        );
    }
}

#[test]
fn test_trusted_reaches_everything() {
    let reaches_all = acls(&policy()).iter().any(|rule| {
        rule_field(rule, "src") == ["tag:trusted"]
            && rule_field(rule, "dst").contains(&"*:*".to_string())
    });
    assert!(
        reaches_all,
        "tag:trusted (lechuck, pixel-9a) is the tier that reaches everything"
    );
}

/// ADR-0052: every enrolled node resolves through the data host's Blocky. The
/// carve-out is `*` -> the resolver on 53 and nothing wider, so it is also the
/// single tailnet path left open to the confined tier.
#[test]
fn test_every_node_reaches_the_global_resolver_and_only_on_53() {
    let dns_rules: Vec<Value> = acls(&policy())
        .into_iter()
        .filter(|rule| rule_field(rule, "src") == ["*"])
        .collect();
    assert_eq!(
        dns_rules.len(),
        1,
        "the only wildcard-source rule is the DNS carve-out"
    );
    assert_eq!(
        rule_field(&dns_rules[0], "dst"),
        [RESOLVER_DST],
        "the carve-out reaches the resolver on 53 only; a wider dst unfilters the tailnet"
    );
    assert_eq!(
        dst_tier(RESOLVER_DST),
        RESOLVER_TIER,
        "the resolver runs on the data tier (ADR-0052)"
    );
}

/// The confined tier initiates nothing of its own — it appears in no rule's
/// `src`. Its only reach is the wildcard DNS rule above, which is exactly the
/// acceptance criterion: a tag:agent node cannot open connections to the data
/// or trusted tiers on anything but 53.
#[test]
fn test_the_agent_tier_initiates_nothing_of_its_own() {
    for rule in acls(&policy()) {
        assert!(
            !rule_field(&rule, "src").contains(&CONFINED_TIER.to_string()),
            "tag:agent names no src rule; a YOLO box initiates no tailnet flow: {rule}"
        );
    }
}

/// The data and standby tiers reach every tier but agent — auberge serves and
/// replicates to the trusted and standby surfaces, and vieille-auberge keeps
/// that reach as the rollback surface. Neither ever initiates toward agent.
#[test]
fn test_data_and_standby_reach_every_tier_but_the_agent() {
    let non_agent: std::collections::BTreeSet<String> = trust_tiers()
        .into_iter()
        .filter(|tier| tier != CONFINED_TIER)
        .collect();
    for src_tier in ["tag:data", "tag:standby"] {
        let rule = acls(&policy())
            .into_iter()
            .find(|rule| rule_field(rule, "src") == [src_tier])
            .unwrap_or_else(|| panic!("{src_tier} must carry an acl rule"));
        let dst_tiers: std::collections::BTreeSet<String> = rule_field(&rule, "dst")
            .iter()
            .map(|entry| dst_tier(entry))
            .collect();
        assert_eq!(
            dst_tiers, non_agent,
            "{src_tier} reaches trusted/data/standby and never {CONFINED_TIER}"
        );
    }
}

/// Every ansible file a deploy renders, as (repo-relative path, contents):
/// every role's templated YAML plus the playbooks ([`templated_yml_files`]
/// already carries the latter), and every role template. A role's `files/`
/// payload is out of scope for the same reason the walker excludes it — ansible
/// copies it byte for byte and nothing there enrolls a node.
///
/// [`ENROLLMENT_SITES`] below are the paths this domain must contain for the
/// scan over it to mean anything.
fn enrollment_domain() -> Vec<(String, String)> {
    templated_yml_files()
        .into_iter()
        .chain(role_templates())
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", relative(&path)));
            (relative(&path), text)
        })
        .collect()
}

/// A node never asserts its own tags. #767 deleted the tailscale role's
/// `tailscale_advertise_tags`; this keeps it deleted.
///
/// The two paths carry opposite authority. A tag stamped on a pre-auth key is
/// server-forced and applied *unchecked* at registration — which is how `ruche`
/// came to carry `tag:agent` on a tailnet running no policy at all. A tag a
/// node advertises through `tailscale up --advertise-tags` is a node-side claim
/// headscale validates against `tagOwners`, and a rejected claim lands as a
/// silently invalid tag on the node record. Two writers for one fact can
/// disagree, only one of them is authoritative, so the other does not exist —
/// and [`TailnetTag`] is now the single declaration the authoritative one reads.
///
/// Text, not structure: a role that merely *documents* the flag is flagged too,
/// which is the right answer while a pre-auth key is the only authority.
#[test]
fn test_a_node_never_advertises_its_own_tags() {
    let offenders: Vec<String> = enrollment_domain()
        .into_iter()
        .filter(|(_, text)| text.contains("advertise-tags") || text.contains("advertise_tags"))
        .map(|(path, _)| path)
        .collect();
    assert!(
        offenders.is_empty(),
        "a pre-auth key is the only thing that assigns a tailnet tag; these assert one \
         node-side: {offenders:?}"
    );
}

/// The two files the scan above must have read — the command line #767 deleted
/// the flag from, and the defaults file it deleted the variable from. A domain
/// that misses either passes vacuously over a place the tag could come back.
const ENROLLMENT_SITES: [&str; 2] = [
    "ansible/roles/tailscale/tasks/main.yml",
    "ansible/roles/tailscale/defaults/main.yml",
];

/// A floor on the domain, well under the ~185 files it holds today. A walk that
/// collapses to a handful still finds no offender, so "no offender" has to be
/// said of a domain that is demonstrably the tree.
const MIN_ENROLLMENT_DOMAIN: usize = 150;

#[test]
fn test_the_enrollment_scan_reaches_both_sites_the_flag_lived_at() {
    let domain = enrollment_domain();
    assert!(
        domain.len() >= MIN_ENROLLMENT_DOMAIN,
        "the scan read only {} files; a collapsed walk finds no offender either",
        domain.len()
    );

    let reached: Vec<&str> = domain.iter().map(|(path, _)| path.as_str()).collect();
    for site in ENROLLMENT_SITES {
        assert!(
            reached.contains(&site),
            "the scan's domain must reach {site}, {} files read",
            domain.len()
        );
    }

    let enrollment_task = ENROLLMENT_SITES[0];
    let text = &domain
        .iter()
        .find(|(path, _)| path == enrollment_task)
        .expect("reached, asserted above")
        .1;
    assert!(
        text.contains("tailscale up"),
        "{enrollment_task} is where the scan is pointed because it enrolls the node; if the \
         command line moved, move this name with it"
    );
}
