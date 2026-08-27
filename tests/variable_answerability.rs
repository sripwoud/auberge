//! Every variable a run renders has somewhere to come from.
//!
//! A role that reads `{{ calibre_subdomain }}` with no default is stating a
//! requirement. Nothing checked the requirement was answerable, so the play ran
//! as far as the first task that needed the name and then died on an undefined
//! variable — on a Host whose `config.toml` never had a chance to carry it,
//! because the Key Registry, the only vocabulary `auberge config init` offers,
//! did not hold the name either (#686).
//!
//! ADR-0045's audit ran this from the registry side: take each of the 62
//! registry names it then held and look for it in the tree. That direction cannot see the
//! case above — a name the registry does not hold is never one of the names
//! iterated, so it is never looked for.
//!
//! This runs the other way. It reads the references first, subtracts everything
//! that can answer one, and asserts nothing is left. Reading only what is
//! *inside* `{{ … }}` and `{% … %}` also drops the collision that direction has
//! with markup — `<hostname>` in `radio/templates/icecast.xml.j2` is an XML tag
//! that a search for the registry name `hostname` hits — structurally rather
//! than by exempting it. A tag is not an expression, so it is never a candidate.
//!
//! ## Answers are scoped to a run, not pooled
//!
//! The unit of judgement is a *run*: one playbook, plus every role it reaches —
//! through its `roles:` list, through those roles' `meta/main.yml` dependencies,
//! and through `include_role`, to a fixpoint. A name is answered if something
//! *that run* provides answers it.
//!
//! Pooling every answer into one set instead is the obvious simplification and
//! it is wrong, because Ansible's narrowest scope is exactly where role
//! parameters are passed. The bash role reads `{{ bash_user_name }}` unguarded
//! and has no `defaults/`; `infrastructure.yml` answers it in a `vars:` on its
//! own `- role: bash` entry, which answers that one invocation and nothing
//! else. `vibecoder.yml` reaches the same role through
//! `vibecoder/meta/main.yml` and bound neither name, so it died mid-play. A
//! pooled sweep sees `bash_user_name` bound somewhere and reports nothing; this
//! one names the run it is unbound in. That was the second defect of
//! `calibre_subdomain`'s shape in the tree, and the reason for the scoping.
//!
//! What each run provides:
//!
//! - the Key Registry — the answer of last resort, and the only one the user
//!   supplies. Registry-wide, since any Host may set any key;
//! - `ansible/group_vars/`, which applies to every Host;
//! - App Versions, injected as `<app>_version` from each Meta's `version:`
//!   block (ADR-0017), and Memory Budgets, injected as `<unit>_memory_high` /
//!   `<unit>_memory_max` from each `memory:` block (ADR-0021);
//! - the `defaults/main.yml` of every role in *this* run;
//! - the playbook's play-level `vars:`, and the `vars:` on each `roles:` entry,
//!   the latter only for the role that entry names;
//! - names the run binds as it goes: `set_fact`, `register`, a task or block
//!   `vars:`, `loop_control.loop_var`, a `{% for %}` target, a `{% set %}`.
//!   Run-wide, because a fact set in one role is visible to the next.
//!
//! Two of those are deliberately looser than Ansible. Run-wide bindings ignore
//! order, so a fact set *after* the role that reads it still answers; role
//! defaults are pooled across the run rather than kept to their own role. Both
//! err toward silence, never toward a false alarm, which is the direction a
//! fence in the build has to err.
//!
//! Ansible's own facts (`ansible_*`, `inventory_hostname`, `item`, …) are named
//! in [`BUILTIN_NAMES`] rather than discovered, because nothing declares them.
//!
//! ## What the sweep found
//!
//! Over 550 distinct unguarded references, 66 registry keys and 10 runs
//! covering all 37 roles, three names, of which two were live deploy failures:
//!
//! - `calibre_subdomain`, read unguarded by `roles/calibre/defaults/main.yml`
//!   and answered by nothing at all;
//! - `bash_user_name` and `bash_user_home`, answered on the `infrastructure.yml`
//!   run and unanswered on the `vibecoder.yml` one.
//!
//! Both are fixed in the commits before this one; reverting either turns this
//! fence red. The third is `paperless_admin_mail`, read once as
//! `| default(admin_user_email)` in `paperless.conf.j2` — guarded, so no run can
//! fail on it, and left alone. Being outside the registry does not make it
//! unsettable, since nothing filters `config.toml` against the registry; it
//! makes it undiscoverable, because `config init` scaffolds registry keys only.
//! A real knob nothing advertises is a documentation gap, not a broken deploy,
//! and not this fence's question.
//!
//! ## Why this is a scanner when ADR-0045 rejected one
//!
//! ADR-0045 considered deriving `required_keys` instead of declaring them and
//! rejected it, because "derivation is blind in exactly the interesting
//! places", naming three: the XML `<hostname>` tag above; `actual_tailscale_ip`,
//! a registry name a role sets by `set_fact` and only accepts as an override;
//! and `baikal_busy_icloud_*`, optional behind `is defined`. A scanner that
//! cannot tell those apart "either demands keys nobody needs or misses keys
//! everybody does".
//!
//! That rejection is of derivation as a *runtime authority* — the thing that
//! decides what a given run demands. Nothing here decides that; the Metas still
//! do, and this never executes on the deploy path. The property asserted is
//! strictly weaker: not "these are calibre's required keys" but "this name has
//! somewhere to come from". A name in the registry passes here whatever any
//! Meta says about it.
//!
//! Weaker is what makes those three survivable, and each is handled rather than
//! tolerated: the XML tag is never read, because only expression bodies are;
//! `actual_tailscale_ip` is answered twice over, by its `set_fact` and by the
//! registry; and the iCloud keys are guarded, so they are not findings. A demand
//! this fence gets wrong costs a test failure and a line of exclusion. A demand
//! Preflight gets wrong costs a deploy.
//!
//! ## What this does not assert
//!
//! That the *right* Meta demands the key. A name answerable only by the Key
//! Registry still has to be in some Meta's `required_keys` or Preflight never
//! asks for it — that pairing is ADR-0045's, and `required_keys_declarations.rs`
//! fences it from the declaration side. Here a name in the registry passes
//! wherever it is declared, or even if it is declared nowhere.

mod common;

use common::{
    all_roles, meta_files, parse_yaml, playbook_files, registry_keys, relative, repo,
    role_template_files, role_templates, role_yml_files, templated_yml_files, yml_files,
};
use regex::Regex;
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Ansible's own variables, which no file in the repo declares.
const BUILTIN_NAMES: &[&str] = &[
    "environment",
    "group_names",
    "groups",
    "hostvars",
    "inventory_dir",
    "inventory_file",
    "inventory_hostname",
    "inventory_hostname_short",
    "item",
    "omit",
    "play_hosts",
    "playbook_dir",
    "role_name",
    "role_path",
    "vars",
];

/// Jinja's own words, plus the callables that appear as bare identifiers.
/// A name here is never a variable reference.
const JINJA_WORDS: &[&str] = &[
    "and",
    "as",
    "block",
    "break",
    "caller",
    "continue",
    "cycler",
    "dict",
    "dictsort",
    "do",
    "elif",
    "else",
    "endblock",
    "endfilter",
    "endfor",
    "endif",
    "endmacro",
    "endraw",
    "endset",
    "endwith",
    "extends",
    "False",
    "false",
    "filter",
    "for",
    "from",
    "if",
    "import",
    "in",
    "include",
    "is",
    "joiner",
    "kwargs",
    "lipsum",
    "list",
    "lookup",
    "loop",
    "macro",
    "namespace",
    "None",
    "none",
    "not",
    "or",
    "q",
    "query",
    "range",
    "raw",
    "recursive",
    "self",
    "set",
    "True",
    "true",
    "undefined",
    "varargs",
    "with",
];

/// YAML keys whose scalar *is* a Jinja expression, with no `{{ … }}` around it.
const BARE_EXPRESSION_KEYS: &[&str] = &[
    "assert",
    "changed_when",
    "failed_when",
    "that",
    "until",
    "when",
];

static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"'[^']*'|"[^"]*""#).unwrap());
static IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap());
static EXPRESSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\{\{(.*?)\}\}|\{%-?(.*?)-?%\}").unwrap());
static FOR_TARGETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\{%-?\s*for\s+(.+?)\s+in\s").unwrap());
static SET_TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{%-?\s*set\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
/// `before` ends in `is` / `is not`, so what follows is a test, not a variable.
static TEST_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bis\s+(not\s+)?$").unwrap());
static DEFAULTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([A-Za-z_][A-Za-z0-9_]*)((?:\s*\.\s*[A-Za-z0-9_]+|\s*\[[^\]]*\])*)\s*\|\s*(default|d)\b",
    )
    .unwrap()
});
static TESTED_DEFINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)((?:\s*\.\s*[A-Za-z0-9_]+)*)\s+is\s+(not\s+)?defined\b")
        .unwrap()
});

/// String literals blanked out, so their contents are never read as names and
/// the offsets around them still line up.
fn mask_literals(expr: &str) -> String {
    let mut masked = String::with_capacity(expr.len());
    let mut cursor = 0;
    for found in LITERAL.find_iter(expr) {
        masked.push_str(&expr[cursor..found.start()]);
        masked.extend(found.as_str().chars().map(|_| ' '));
        cursor = found.end();
    }
    masked.push_str(&expr[cursor..]);
    masked
}

/// Every Jinja expression body in a blob of text.
fn expressions(text: &str) -> Vec<String> {
    EXPRESSION
        .captures_iter(text)
        .filter_map(|caps| caps.get(1).or_else(|| caps.get(2)))
        .map(|m| m.as_str().to_string())
        .collect()
}

/// The names a template binds for itself: `{% for x, y in … %}`, `{% set x = … %}`.
fn template_bindings(text: &str) -> BTreeSet<String> {
    let mut bound = BTreeSet::new();
    for caps in FOR_TARGETS.captures_iter(text) {
        for part in caps[1].split(',') {
            let part = part.trim().trim_matches(['(', ')']).trim();
            if IDENT.find(part).is_some_and(|m| m.as_str() == part) {
                bound.insert(part.to_string());
            }
        }
    }
    for caps in SET_TARGET.captures_iter(text) {
        bound.insert(caps[1].to_string());
    }
    bound
}

/// The variables an expression reads at their root.
///
/// An identifier is a root reference unless something in front of it says
/// otherwise: `.` makes it an attribute, `|` a filter, `is` a test, and a
/// following `=` a callable's keyword argument.
fn root_names(expr: &str) -> BTreeSet<String> {
    let masked = mask_literals(expr);
    let mut names = BTreeSet::new();
    for found in IDENT.find_iter(&masked) {
        let before = &masked[..found.start()];
        let trimmed = before.trim_end();
        if trimmed.ends_with('.') || trimmed.ends_with('|') || TEST_NAME.is_match(before) {
            continue;
        }
        let after = masked[found.end()..].trim_start();
        if after.starts_with('=') && !after.starts_with("==") {
            continue;
        }
        let name = found.as_str();
        if JINJA_WORDS.contains(&name) {
            continue;
        }
        names.insert(name.to_string());
    }
    names
}

/// The names this expression answers for itself: `x | default(…)`, and
/// `x is defined`, which is how a role reads an optional key.
fn guarded_names(expr: &str) -> BTreeSet<String> {
    let masked = mask_literals(expr);
    let mut guarded = BTreeSet::new();
    for caps in DEFAULTED.captures_iter(&masked) {
        guarded.insert(caps[1].to_string());
    }
    for caps in TESTED_DEFINED.captures_iter(&masked) {
        guarded.insert(caps[1].to_string());
    }
    guarded
}

/// What one file says: the names it reads with no guard, the names it binds,
/// and the roles it pulls in by name.
#[derive(Default)]
struct FileScan {
    unguarded: BTreeSet<String>,
    bound: BTreeSet<String>,
    included_roles: BTreeSet<String>,
}

impl FileScan {
    /// Text that may hold `{{ … }}` / `{% … %}` among literal content.
    fn read_text(&mut self, text: &str) {
        self.bound.extend(template_bindings(text));
        for expr in expressions(text) {
            let guarded = guarded_names(&expr);
            for name in root_names(&expr) {
                if !guarded.contains(&name) {
                    self.unguarded.insert(name);
                }
            }
        }
    }

    /// A scalar that is itself an expression — a `when:` and its siblings.
    fn read_expression(&mut self, expr: &str) {
        if expr.contains("{{") || expr.contains("{%") {
            self.read_text(expr);
            return;
        }
        let guarded = guarded_names(expr);
        for name in root_names(expr) {
            if !guarded.contains(&name) {
                self.unguarded.insert(name);
            }
        }
    }

    fn walk(&mut self, node: &Value, bare: bool) {
        match node {
            Value::Mapping(map) => {
                for (key, value) in map {
                    let Some(key) = key.as_str() else { continue };
                    // `ansible.builtin.set_fact` and `set_fact` are one module.
                    match key.rsplit('.').next().unwrap_or(key) {
                        "set_fact" => self.bind_mapping_keys(value, &["cacheable"]),
                        "vars" => self.bind_mapping_keys(value, &[]),
                        "register" => {
                            if let Some(name) = value.as_str() {
                                self.bound.insert(name.to_string());
                            }
                        }
                        "loop_control" => {
                            if let Some(name) = value.get("loop_var").and_then(Value::as_str) {
                                self.bound.insert(name.to_string());
                            }
                        }
                        "include_role" | "import_role" => {
                            if let Some(name) = value.get("name").and_then(Value::as_str)
                                && !name.contains("{{")
                            {
                                self.included_roles.insert(name.to_string());
                            }
                        }
                        _ => {}
                    }
                    self.walk(value, BARE_EXPRESSION_KEYS.contains(&key));
                }
            }
            Value::Sequence(items) => {
                for item in items {
                    self.walk(item, bare);
                }
            }
            Value::String(text) => {
                if bare {
                    self.read_expression(text);
                } else {
                    self.read_text(text);
                }
            }
            _ => {}
        }
    }

    fn bind_mapping_keys(&mut self, value: &Value, skip: &[&str]) {
        let Value::Mapping(map) = value else { return };
        for key in map.keys() {
            if let Some(name) = key.as_str()
                && !skip.contains(&name)
            {
                self.bound.insert(name.to_string());
            }
        }
    }
}

fn scan_yaml(path: &Path) -> FileScan {
    let mut scan = FileScan::default();
    scan.walk(&parse_yaml(path), false);
    scan
}

fn scan_template(path: &Path) -> FileScan {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", relative(path)));
    let mut scan = FileScan::default();
    scan.read_text(&raw);
    scan
}

fn scan(path: &Path) -> FileScan {
    if path.extension().is_some_and(|ext| ext == "yml") {
        scan_yaml(path)
    } else {
        scan_template(path)
    }
}

/// Every file of a role a deploy renders.
fn role_files(role: &str) -> Vec<PathBuf> {
    let mut files = role_yml_files(role);
    files.extend(role_template_files(role));
    files
}

fn top_level_keys(path: &Path) -> BTreeSet<String> {
    if !path.exists() {
        return BTreeSet::new();
    }
    match parse_yaml(path) {
        Value::Mapping(map) => map
            .keys()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// The roles one role declares as dependencies in its `meta/main.yml`.
fn declared_dependencies(role: &str) -> BTreeSet<String> {
    let path = common::role_dir(role).join("meta/main.yml");
    if !path.exists() {
        return BTreeSet::new();
    }
    let Value::Mapping(meta) = parse_yaml(&path) else {
        return BTreeSet::new();
    };
    meta.get("dependencies")
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(|entry| match entry {
                    Value::String(name) => Some(name.clone()),
                    other => other
                        .get("role")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One playbook and every role it reaches.
struct Run {
    playbook: PathBuf,
    roles: BTreeSet<String>,
    /// Play-level `vars:`, which answer every role in the run.
    play_vars: BTreeSet<String>,
    /// `vars:` on a `roles:` entry, which answer only the role it names.
    entry_vars: BTreeMap<String, BTreeSet<String>>,
    /// Names the run binds as it goes, from the playbook and every role in it.
    bound: BTreeSet<String>,
}

/// Grow a role set through `meta/main.yml` dependencies and `include_role`
/// until it stops growing. Both are static edges in the tree; a name built by
/// interpolation is skipped, and no role in the repo is included that way.
fn reachable_roles(seed: BTreeSet<String>) -> BTreeSet<String> {
    let known = all_roles();
    let mut roles = seed;
    loop {
        let mut grown = roles.clone();
        for role in &roles {
            if !known.contains(role) {
                continue;
            }
            grown.extend(declared_dependencies(role));
            for path in role_files(role) {
                grown.extend(scan(&path).included_roles);
            }
        }
        if grown == roles {
            return roles;
        }
        roles = grown;
    }
}

fn runs() -> Vec<Run> {
    let mut runs = Vec::new();
    for playbook in playbook_files() {
        let plays = parse_yaml(&playbook);
        let Some(plays) = plays.as_sequence() else {
            panic!("{} must hold a list of plays", relative(&playbook))
        };
        let mut seed = BTreeSet::new();
        let mut play_vars = BTreeSet::new();
        let mut entry_vars: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for play in plays {
            play_vars.extend(
                play.get("vars")
                    .and_then(Value::as_mapping)
                    .map(|vars| {
                        vars.keys()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<BTreeSet<String>>()
                    })
                    .unwrap_or_default(),
            );
            for entry in play
                .get("roles")
                .and_then(Value::as_sequence)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let name = match entry {
                    Value::String(name) => name.clone(),
                    other => match other.get("role").and_then(Value::as_str) {
                        Some(name) => name.to_string(),
                        None => continue,
                    },
                };
                if let Some(vars) = entry.get("vars").and_then(Value::as_mapping) {
                    entry_vars
                        .entry(name.clone())
                        .or_default()
                        .extend(vars.keys().filter_map(Value::as_str).map(str::to_string));
                }
                seed.insert(name);
            }
        }

        let playbook_scan = scan(&playbook);
        seed.extend(playbook_scan.included_roles.clone());
        let roles = reachable_roles(seed);

        let mut bound = playbook_scan.bound.clone();
        for role in &roles {
            for path in role_files(role) {
                bound.extend(scan(&path).bound);
            }
        }

        runs.push(Run {
            playbook,
            roles,
            play_vars,
            entry_vars,
            bound,
        });
    }
    assert!(!runs.is_empty(), "no playbook was read at all");
    runs
}

/// The answers that hold for every run: the Key Registry, `group_vars/`, and
/// the names the CLI injects off the Playbook Metas.
fn universal_answers() -> BTreeSet<String> {
    let mut answers = registry_keys();
    for path in group_vars_files() {
        answers.extend(top_level_keys(&path));
    }
    for (app, path) in meta_files() {
        let Value::Mapping(meta) = parse_yaml(&path) else {
            continue;
        };
        if meta.get("version").is_some() {
            answers.insert(format!("{app}_version"));
        }
        if let Some(budgets) = meta.get("memory").and_then(Value::as_mapping) {
            for unit in budgets.keys().filter_map(Value::as_str) {
                let prefix = unit.replace('-', "_");
                answers.insert(format!("{prefix}_memory_high"));
                answers.insert(format!("{prefix}_memory_max"));
            }
        }
    }
    answers
}

fn group_vars_files() -> Vec<PathBuf> {
    let dir = repo().join("ansible").join("group_vars");
    let found = yml_files(&dir);
    assert!(
        !found.is_empty(),
        "{} holds no .yml; it answers names no role declares",
        relative(&dir)
    );
    found
}

fn answered(name: &str) -> bool {
    name.starts_with("ansible_") || BUILTIN_NAMES.contains(&name)
}

/// Every unanswered `(name, where)` pair across every run.
fn unanswered() -> Vec<(String, String)> {
    let universal = universal_answers();
    let mut gaps: BTreeSet<(String, String)> = BTreeSet::new();

    for run in runs() {
        let mut run_answers = universal.clone();
        run_answers.extend(run.play_vars.iter().cloned());
        run_answers.extend(run.bound.iter().cloned());
        for role in &run.roles {
            run_answers.extend(top_level_keys(
                &common::role_dir(role).join("defaults/main.yml"),
            ));
        }

        let playbook_name = relative(&run.playbook);
        for name in &scan(&run.playbook).unguarded {
            if !answered(name) && !run_answers.contains(name) {
                gaps.insert((name.clone(), format!("{playbook_name} (play)")));
            }
        }
        for role in &run.roles {
            let empty = BTreeSet::new();
            let entry = run.entry_vars.get(role).unwrap_or(&empty);
            for path in role_files(role) {
                for name in &scan(&path).unguarded {
                    if answered(name) || run_answers.contains(name) || entry.contains(name) {
                        continue;
                    }
                    gaps.insert((
                        name.clone(),
                        format!("{playbook_name} -> {}", relative(&path)),
                    ));
                }
            }
        }
    }
    gaps.into_iter().collect()
}

#[test]
fn test_every_unguarded_variable_a_run_reads_has_an_answer() {
    let gaps = unanswered();
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, site) in &gaps {
        by_name.entry(name).or_default().push(site);
    }
    let report: Vec<String> = by_name
        .iter()
        .map(|(name, sites)| format!("  {name} — unanswered on {}", sites.join(", ")))
        .collect();

    assert!(
        report.is_empty(),
        "{} variable(s) are read with no default and nothing in the run answers \
         them — not a role default, not group_vars, not the playbook's vars, not \
         a name the run binds, not an App Version or Memory Budget, and not the \
         Key Registry. Each named run dies mid-play on that variable. Either \
         give the reference a `| default(…)`, bind it where the run passes the \
         role its parameters, or add the key to ansible/keys.yml and declare it \
         in the App's Meta:\n{}",
        by_name.len(),
        report.join("\n")
    );
}

#[test]
fn test_every_role_is_reached_by_some_run() {
    // A role no run reaches is a role this fence says nothing about, and it
    // would say nothing silently.
    let reached: BTreeSet<String> = runs().into_iter().flat_map(|run| run.roles).collect();
    let orphans: Vec<String> = all_roles()
        .into_iter()
        .filter(|role| !reached.contains(role))
        .collect();

    assert!(
        orphans.is_empty(),
        "role(s) no playbook reaches, so no run checks their variables: {}. \
         Either a playbook lost them, or they are reached by an edge this walk \
         does not follow — a `roles:` entry, a `meta/main.yml` dependency, or a \
         static `include_role`",
        orphans.join(", ")
    );
}

#[test]
fn test_the_sweep_still_reaches_the_tree_it_reasons_about() {
    // A sweep that stops reaching does not fail — it shrinks its domain and
    // goes on passing, vacuously. These are the sizes measured when the sweep
    // found `calibre_subdomain` and `bash_user_name` (#686), as floors: the
    // tree grows, and a fall means the walk stopped seeing part of it.
    let yml = templated_yml_files();
    let templates = role_templates();
    let runs = runs();
    let registry = registry_keys();

    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for path in yml.iter().chain(templates.iter()) {
        referenced.extend(scan(path).unguarded);
    }
    let registry_read = registry.iter().filter(|k| referenced.contains(*k)).count();
    let widest = runs
        .iter()
        .map(|run| run.roles.len())
        .max()
        .expect("there is at least one run");

    assert!(
        yml.len() >= 100,
        "templated YAML dropped to {} files, was 107",
        yml.len()
    );
    assert!(
        templates.len() >= 70,
        "role templates dropped to {} files, was 77",
        templates.len()
    );
    assert!(
        referenced.len() >= 500,
        "distinct unguarded names referenced dropped to {}, was 550",
        referenced.len()
    );
    assert!(
        registry_read >= 55,
        "registry keys any role reads unguarded dropped to {registry_read}, was 60"
    );
    assert!(
        runs.len() >= 10,
        "runs dropped to {}, was 10 — a playbook stopped being read",
        runs.len()
    );
    assert!(
        widest >= 18,
        "the widest run reaches {widest} roles, was 20 — a roster or a \
         dependency edge stopped resolving"
    );
}
