//! The OpenCode permission baseline, asserted the way the runtime reads it.
//!
//! ADR-0065 runs agents unattended with permissions pre-approved, which makes
//! prompt injection the design assumption. What is left standing between an
//! injected agent and an irreversible act is a JSON file, and what makes such a
//! file hard to get right is that it is not evaluated the way it is written.
//!
//! OpenCode flattens the whole `permission` object into **one** list of
//! `{permission, pattern}` rules in object key order, and decides with
//! `findLast` — the last rule whose permission matches the tool *and* whose
//! pattern matches the input. Three consequences, none of them visible while
//! reading the file as nested blocks, and all three are what this fence is for:
//!
//! - A top-level `"*": "allow"` is not "the default for tools I did not
//!   mention". It is a rule matching **every tool and every input**, and if it
//!   sorts after the `bash` block it allows everything that block denies. It
//!   sorts there as soon as any lower-precedence config mentions
//!   `permission.bash` at all — config sources merge as `{...lower, ...managed}`,
//!   so a key the lower file also has keeps the lower file's *position* while
//!   taking the managed file's *value*, and every managed-only key is appended
//!   after it. Measured against the shipped 1.18.25 binary: a project config of
//!   `{"permission":{"bash":{"harmless-noop":"deny"}}}` moved the top-level rule
//!   from index 0 to index 17, behind all sixteen denies. The baseline carries no
//!   cross-tool rule for that reason, and [`test_no_rule_matches_across_tools`]
//!   is what keeps one from coming back.
//!
//! - Within a block the catch-all has to come first, for the same reason at a
//!   smaller scale.
//!
//! - The presence of a rule proves nothing about the verdict. So the assertions
//!   run concrete commands through a model of `findLast` over the flattened
//!   list, rather than checking that a pattern appears in the file.
//!
//! What this fence does **not** claim is that the baseline cannot be lifted. It
//! can: a config source the agent can write that names a deny pattern verbatim
//! hoists that key above the block's own catch-all and inverts it for that one
//! pattern. ADR-0065 records the measurement and names where the real boundaries
//! are. What is fenced here is the property that survives — the baseline is
//! correct in isolation, and no benign config elsewhere flips it wholesale.
//!
//! Rendered with minijinja under `UndefinedBehavior::Strict` and `trim_blocks`,
//! matching ansible: a name nothing answers must fail here rather than render as
//! the empty string and compare equal to a path that was never written.

use std::collections::BTreeMap;
use std::fs;

use serde_json::Value as Json;
use serde_yaml::Value;

mod common;

use common::{defaults, field, relative, repo, resolve, role_dir, role_tasks, task_name};

const ROLE: &str = "opencode";

/// The managed config directory OpenCode reads on Linux, and the files in it.
/// Stated as literals rather than read back from the role's defaults: a fence
/// that resolved the role's own variable would follow the role wherever it moved
/// and assert nothing.
const MANAGED_CONFIG: &str = "/etc/opencode/opencode.json";
const KEY_FILE: &str = "/etc/opencode/openrouter.key";

/// The admin user a run binds; the role's paths resolve through it and the Key
/// Registry answers it, so the render needs a value and any value will do.
const ADMIN_USER: &str = "agent";

fn env() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    // Ansible templates with `trim_blocks` on; minijinja defaults it off, and a
    // fence that renders the other way asserts over text no deploy produces.
    env.set_trim_blocks(true);
    env
}

/// The role's scalar defaults plus the one name a run supplies. Both the render
/// and the `dest:`/`mode:` reads resolve through this, so a path stated in a task
/// and the same path stated in the config arrive here as the same text.
fn vars() -> BTreeMap<String, String> {
    let mut vars = defaults(ROLE);
    vars.insert("admin_user_name".to_string(), ADMIN_USER.to_string());
    vars
}

/// [`vars`] resolved against itself, plus the structured defaults the scalar
/// walk drops.
fn context() -> BTreeMap<String, minijinja::Value> {
    let vars = vars();
    vars.iter()
        .map(|(key, raw)| (key.clone(), minijinja::Value::from(resolve(raw, &vars))))
        .collect()
}

fn template_path() -> std::path::PathBuf {
    role_dir(ROLE).join("templates/opencode.json.j2")
}

fn render(plugins: &[&str]) -> String {
    let path = template_path();
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)));
    let mut ctx = context();
    ctx.insert(
        "opencode_plugins".to_string(),
        minijinja::Value::from(
            plugins
                .iter()
                .map(|p| minijinja::Value::from(*p))
                .collect::<Vec<_>>(),
        ),
    );
    env()
        .render_str(&raw, ctx)
        .unwrap_or_else(|e| panic!("{} must render: {e}", relative(&path)))
}

/// The config the role deploys, rendered and parsed. Parsing is half the point:
/// a hand-written JSON template renders to a string whatever it does to the
/// braces, and a deploy is where a trailing comma would otherwise surface.
fn config() -> Json {
    let rendered = render(&[]);
    serde_json::from_str(&rendered)
        .unwrap_or_else(|e| panic!("the baseline must render to valid JSON: {e}\n{rendered}"))
}

/// Every rule the runtime evaluates, flattened and in evaluation order:
/// `(permission, pattern, action)`.
///
/// Read off the rendered text rather than the parsed value, because the order is
/// what decides the outcome and `serde_json` does not preserve it without the
/// `preserve_order` feature. A scalar entry (`"external_directory": "allow"`) is
/// a rule with pattern `*`; a top-level entry is a rule whose *permission* is the
/// key, which is how a `"*"` there comes to match every tool.
fn flat_rules() -> Vec<(String, String, String)> {
    let rendered = render(&[]);
    let anchor = "\"permission\": {";
    let start = rendered
        .find(anchor)
        .expect("the baseline must declare a `permission` object")
        + anchor.len();
    let unquote = |s: &str| s.trim().trim_matches('"').to_string();

    let mut rules = Vec::new();
    let mut tool: Option<String> = None;
    let mut closed = false;
    for line in rendered[start..].lines() {
        let line = line.trim().trim_end_matches(',');
        if line == "}" {
            match tool.take() {
                Some(_) => continue,
                None => {
                    closed = true;
                    break;
                }
            }
        }
        if let Some(name) = line.strip_suffix(": {") {
            tool = Some(unquote(name));
            continue;
        }
        let Some((key, verdict)) = line.rsplit_once(": ") else {
            continue;
        };
        match &tool {
            Some(name) => rules.push((name.clone(), unquote(key), unquote(verdict))),
            None => rules.push((unquote(key), "*".to_string(), unquote(verdict))),
        }
    }
    assert!(
        closed,
        "the `permission` object did not close; this walk no longer sees the shape \
         it reasons about"
    );
    assert!(
        rules.len() >= 20,
        "only {} rules were read off the baseline; the walk stopped seeing it",
        rules.len()
    );
    rules
}

/// OpenCode's wildcard matcher: `*` is zero or more of any character, `?` is
/// exactly one, everything else is literal.
fn matches(pattern: &str, input: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let c: Vec<char> = input.chars().collect();
    let (mut pi, mut ci) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ci < c.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == c[ci]) {
            pi += 1;
            ci += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ci;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ci = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The verdict the baseline gives `input` on `tool`: the **last** rule matching
/// both, which is what `findLast` does. `None` means no rule matched and
/// OpenCode's own default decides.
fn decide(tool: &str, input: &str) -> Option<String> {
    flat_rules()
        .into_iter()
        .filter(|(permission, pattern, _)| matches(permission, tool) && matches(pattern, input))
        .next_back()
        .map(|(_, _, verdict)| verdict)
}

/// The rules of one tool block, in file order.
fn block(tool: &str) -> Vec<(String, String)> {
    let rules: Vec<(String, String)> = flat_rules()
        .into_iter()
        .filter(|(permission, _, _)| permission == tool)
        .map(|(_, pattern, verdict)| (pattern, verdict))
        .collect();
    assert!(
        !rules.is_empty(),
        "the baseline must carry a `{tool}` block"
    );
    rules
}

/// One task of the role, found by its `name:`.
fn task(name: &str) -> serde_yaml::Mapping {
    role_tasks(ROLE)
        .into_iter()
        .find(|t| task_name(&t.body) == name)
        .unwrap_or_else(|| panic!("the {ROLE} role must have a task named `{name}`"))
        .body
}

fn arg(task: &serde_yaml::Mapping, module: &str, key: &str) -> String {
    let args = field(task, module)
        .and_then(Value::as_mapping)
        .unwrap_or_else(|| panic!("the task must call {module}"));
    let raw = field(args, key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{module} must declare `{key}`"));
    resolve(raw, &vars())
}

/// The defect this fence was written for, and the one that is invisible in the
/// file: a rule whose *permission* is a wildcard matches every tool, so its
/// position relative to the `bash` block decides whether sixteen denies mean
/// anything. Its position is not this repo's to control — it moves when any
/// lower-precedence config mentions the same tool — so the rule must not exist.
#[test]
fn test_no_rule_matches_across_tools() {
    let cross: Vec<String> = flat_rules()
        .into_iter()
        .filter(|(permission, _, _)| permission.contains(['*', '?']))
        .map(|(permission, pattern, verdict)| format!("{permission}/{pattern} -> {verdict}"))
        .collect();
    assert!(
        cross.is_empty(),
        "the baseline declares a rule matching across tools: {}. Config sources \
         merge as {{...lower, ...managed}}, so a key any lower config also names \
         keeps that file's position and every managed-only key lands after it — \
         which sinks a top-level `\"*\"` below the whole deny list and allows \
         everything it denies. State the blanket per tool instead; OpenCode's own \
         defaults already allow what neither block names",
        cross.join(", ")
    );
}

#[test]
fn test_each_block_states_its_catch_all_first() {
    for tool in ["bash", "read"] {
        assert_eq!(
            block(tool).first().map(|(p, v)| (p.as_str(), v.as_str())),
            Some(("*", "allow")),
            "`{tool}`'s catch-all must come first: the last matching rule wins, so \
             a `\"*\": \"allow\"` below the deny list re-allows every rule above it \
             while the file still reads as though it denies them"
        );
    }
}

#[test]
fn test_the_unattended_lane_approves_ordinary_work() {
    for command in [
        "cargo test",
        "git status --porcelain",
        "git push origin feature-branch",
        "git push origin HEAD:refs/heads/topic",
        "git commit -m 'wip'",
        "rm -rf node_modules",
        "rm -rf ./target",
        "gh pr create --fill",
        "dd if=/dev/urandom of=./fixture.bin",
    ] {
        assert_eq!(
            decide("bash", command).as_deref(),
            Some("allow"),
            "`{command}` must run unattended; a baseline that stops ordinary work \
             is a baseline someone disables"
        );
    }
}

#[test]
fn test_the_irreversible_acts_are_denied() {
    for command in [
        // History rewrites, in every spelling: the flag, its short form, and the
        // `+refspec` form that carries no flag at all.
        "git push --force",
        "git push --force-with-lease",
        "git push -f",
        "git push origin main --force",
        "git push origin main -f",
        "git push origin +main:main",
        "git push +main",
        // Escalation.
        "sudo tee /etc/opencode/opencode.json",
        "sudo rm /etc/opencode/openrouter.key",
        "doas sh",
        // Destruction that outlives a rebuild of the worktree.
        "rm -rf /",
        "rm -rf /etc",
        "rm -fr /var/lib",
        "rm -rf ~/.ssh",
        "rm -fr ~/.ssh",
        "rm -rf $HOME",
        "rm -fr $HOME",
        "rm -rf --no-preserve-root /",
        "mkfs.ext4 /dev/sda1",
        "dd if=/dev/zero of=/dev/sda",
        "dd of=/dev/sda if=/dev/zero",
    ] {
        assert_eq!(
            decide("bash", command).as_deref(),
            Some("deny"),
            "`{command}` must be denied by the baseline"
        );
    }
}

/// The two permissions OpenCode defaults to `ask` are the two an unattended lane
/// cannot leave alone: `external_directory` fires for any path outside the
/// session's own directory, which a worktree-per-agent layout crosses constantly.
#[test]
fn test_nothing_the_lane_needs_is_left_asking() {
    let asking: Vec<String> = flat_rules()
        .into_iter()
        .filter(|(_, _, verdict)| verdict == "ask")
        .map(|(permission, pattern, _)| format!("{permission}/{pattern}"))
        .collect();
    assert!(
        asking.is_empty(),
        "nobody is watching the session, so an `ask` is a hang until someone \
         answers it from a phone: {}",
        asking.join(", ")
    );
    assert_eq!(
        decide("external_directory", "/home/agent/workspace/other-repo").as_deref(),
        Some("allow"),
        "agents work across worktrees under a shared root; OpenCode defaults this \
         to `ask`, so the baseline has to say otherwise"
    );
}

#[test]
fn test_reads_that_would_leak_a_secret_are_denied_by_default() {
    // Transcripts leave the box (ADR-0054), so a read is an egress. This is a
    // default, not a boundary — `bash` is allow-all and `cat` reaches both of
    // these. It is asserted because losing it silently is the failure mode:
    // OpenCode denies `.env` out of the box, and a block-level `"*": "allow"`
    // above would otherwise drop that default on the floor.
    for path in ["/srv/app/.env", "/srv/app/.env.local", KEY_FILE] {
        assert_eq!(
            decide("read", path).as_deref(),
            Some("deny"),
            "reading `{path}` must be denied; what the agent reads is archived \
             off-host"
        );
    }
    assert_eq!(
        decide("read", "/srv/app/.env.example").as_deref(),
        Some("allow"),
        "the `.env.example` carve-out is upstream's and worth keeping"
    );
}

#[test]
fn test_the_baseline_lands_where_a_project_config_cannot_replace_it() {
    let deploy = task("Deploy the managed OpenCode configuration");
    assert_eq!(
        arg(&deploy, "ansible.builtin.template", "dest"),
        MANAGED_CONFIG,
        "the baseline must land in OpenCode's managed config directory, the only \
         tier whose values outrank a project or user config"
    );
    assert_eq!(
        arg(&deploy, "ansible.builtin.template", "owner"),
        "root",
        "a baseline the agent's own user owns is a baseline it can edit outright"
    );
    assert_eq!(
        arg(&deploy, "ansible.builtin.template", "mode"),
        "0644",
        "group- or world-writable is the same hole as agent-owned"
    );
}

#[test]
fn test_the_openrouter_key_is_a_reference_and_never_a_value() {
    let api_key = config()["provider"]["openrouter"]["options"]["apiKey"]
        .as_str()
        .expect("the baseline must configure the openrouter provider's apiKey")
        .to_string();
    assert_eq!(
        api_key,
        format!("{{file:{KEY_FILE}}}"),
        "the key must reach OpenCode as a `{{file:…}}` substitution against its \
         own 0640 sidecar; interpolating the variable here deploys and works, and \
         writes the credential into a 0644 file"
    );

    let template = fs::read_to_string(template_path()).unwrap();
    assert!(
        !template.contains("opencode_openrouter_api_key"),
        "the config template must not name the key variable at all — the sidecar \
         is the only place it is rendered"
    );

    let write = task("Deploy the OpenRouter API key");
    assert_eq!(
        arg(&write, "ansible.builtin.copy", "dest"),
        KEY_FILE,
        "the sidecar the config references and the sidecar the role writes must be \
         the same file"
    );
    assert_eq!(
        arg(&write, "ansible.builtin.copy", "mode"),
        "0640",
        "the sidecar carries the only model credential on the Host"
    );
    assert_eq!(
        arg(&write, "ansible.builtin.copy", "owner"),
        "root",
        "root-owned, agent-readable: OpenCode runs as the agent and needs the key, \
         so this bounds who may replace it, not who may read it"
    );
    assert_eq!(
        field(&write, "no_log").and_then(Value::as_bool),
        Some(true),
        "a task that renders the key must not echo it into the deploy log"
    );
}

/// ADR-0045: no secret is a repo literal. Asserted over the whole tree rather
/// than the role, because the failure this catches is a key pasted into a doc, an
/// example config or a test fixture — never into the role that needs it.
#[test]
fn test_no_openrouter_credential_is_committed() {
    // Assembled rather than spelled out: a fence that writes the prefix
    // literally matches itself the moment it is committed, and the obvious
    // repair -- excluding this file -- would exempt the one file most likely to
    // grow a fixture key.
    let prefix = concat!("sk", "-or-");
    let versioned = concat!("sk", "-or-", "v1-");
    let output = std::process::Command::new("git")
        .args(["grep", "-lI", "-e", versioned, "-e", prefix])
        .current_dir(repo())
        .output()
        .expect("git grep must run");
    let hits = String::from_utf8_lossy(&output.stdout);
    let hits: Vec<&str> = hits.lines().filter(|line| !line.is_empty()).collect();
    assert!(
        hits.is_empty(),
        "an OpenRouter key prefix is committed in: {}",
        hits.join(", ")
    );
}

/// #744 adds `@zilliz/memsearch-opencode` to this config. The seam it extends
/// through is the `opencode_plugins` default, so that change is a list entry and
/// never an edit to the permission model beside it.
#[test]
fn test_the_plugin_list_is_a_seam_and_not_a_literal() {
    let template = fs::read_to_string(template_path()).unwrap();
    assert!(
        template.contains("opencode_plugins"),
        "the `plugin` list must render from `opencode_plugins`, so a runtime \
         addition (#744) is one default and not a rewrite of this file"
    );
    assert_eq!(
        config()["plugin"],
        Json::Array(vec![]),
        "this role declares no plugin of its own"
    );

    let rendered = render(&["@zilliz/memsearch-opencode", "opencode-other"]);
    let extended: Json = serde_json::from_str(&rendered).unwrap_or_else(|e| {
        panic!("a populated plugin list must stay valid JSON: {e}\n{rendered}")
    });
    assert_eq!(
        extended["plugin"],
        serde_json::json!(["@zilliz/memsearch-opencode", "opencode-other"]),
        "the seam must carry the list through verbatim"
    );
    assert_eq!(
        extended["permission"],
        config()["permission"],
        "extending the plugin list must not disturb the permission model"
    );
}

/// A baseline nothing loads is a baseline in name only: OpenCode reads
/// `AGENTS.md` by walking up from the session's directory, which a worktree with
/// its own `AGENTS.md` short-circuits. Naming the file in `instructions:` is what
/// makes it apply to every session regardless of where one starts.
#[test]
fn test_the_agents_baseline_is_deployed_and_actually_loaded() {
    let deploy = task("Deploy the AGENTS.md baseline to the workspace root");
    let dest = arg(&deploy, "ansible.builtin.copy", "dest");
    let instructions = config()["instructions"]
        .as_array()
        .expect("the baseline must declare `instructions`")
        .iter()
        .filter_map(Json::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(
        instructions.contains(&dest),
        "the role deploys AGENTS.md to {dest}, which `instructions` does not name \
         ({instructions:?}); a session started outside the workspace would never \
         read it"
    );
    assert!(
        dest.ends_with("/AGENTS.md"),
        "the workspace baseline must be an AGENTS.md so the upward walk finds it \
         too, not only the `instructions:` entry"
    );

    let source = role_dir(ROLE).join("files/AGENTS.md");
    let text = fs::read_to_string(&source).unwrap_or_else(|e| panic!("{}: {e}", relative(&source)));
    assert!(
        !text.trim().is_empty(),
        "{} is empty; every session loads it",
        relative(&source)
    );
}

/// The acceptance criterion #741 states in full: bumping the pin has to replace
/// the artifact on the Host.
///
/// Upstream's linux asset is `opencode-linux-x64.tar.gz` — the same filename at
/// every version. `get_url` with the default `force: false` is a conditional GET,
/// not a skip-if-exists, so a fixed `/tmp` dest holding the previous release can
/// answer 304 and leave the bump downloading nothing (#595, and the trap the
/// issue's brief named). Two independent things close it and both are asserted,
/// because either alone is one edit from being removed by someone tidying the
/// other.
#[test]
fn test_a_version_bump_cannot_reuse_a_stale_download() {
    let download = task("Download OpenCode release archive");
    let dest = field(&download, "ansible.builtin.get_url")
        .and_then(Value::as_mapping)
        .and_then(|args| field(args, "dest"))
        .and_then(Value::as_str)
        .expect("the download must name a dest");
    let dest = resolve(dest, &vars());
    assert!(
        dest.contains("{{ opencode_version }}"),
        "the download dest is `{dest}`, which does not carry the App Version. \
         Upstream's asset filename is the same at every release, so a fixed dest \
         makes the bump a conditional GET against the previous version's bytes"
    );

    let removed = task("Remove the downloaded OpenCode archive");
    assert_eq!(
        arg(&removed, "ansible.builtin.file", "path"),
        dest,
        "the archive the role deletes must be the one it downloaded"
    );
    assert!(
        removed.get(Value::from("when")).is_none(),
        "the cleanup must not inherit the install's guard: an archive left behind \
         by an interrupted run is exactly what a later conditional GET would reuse"
    );
}

/// The installed version is read back off the artifact, never off a note the role
/// wrote (ADR-0027, the artifact-read regime). `install_guards.rs` fences the
/// marker regime structurally and says nothing about a role that has no marker;
/// this says which regime this role is in, so a future edit that introduces one
/// is a deliberate move rather than a drift.
#[test]
fn test_the_installed_version_is_read_from_the_binary() {
    let read = task("Read installed OpenCode version");
    let argv = field(&read, "ansible.builtin.command")
        .and_then(Value::as_str)
        .expect("the version read must be a command");
    assert_eq!(
        resolve(argv, &vars()),
        "/usr/local/bin/opencode --version",
        "the guard must interrogate the installed binary; a deleted or hand-swapped \
         artifact has to read as nothing installed"
    );
    assert_eq!(
        field(&read, "failed_when").and_then(Value::as_bool),
        Some(false),
        "a fresh Host has no binary to run, and that is not a deploy failure"
    );
}
