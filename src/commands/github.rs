//! `auberge github` — provisions the fleet's GitHub identity as a machine
//! user, not the owner (ADR-0054, #745).
//!
//! Everything below the [`Gh`] seam speaks the GitHub CLI's own command line,
//! run locally as the operator: `invite` runs as the owner (ambient `gh`
//! auth), `verify` runs as the bot by passing its token through `GH_TOKEN`.
//! The two scriptable verbs live here; account signup and fine-grained-token
//! minting are irreducibly manual and stay a docs checklist.
//!
//! The per-repo allowlist (ADR-0054) is enforced at the *collaboration* layer
//! — who the bot is invited to — not the token's scope. `verify` proves push
//! access by reading `.permissions.push` off `GET /repos/{repo}`: a public
//! owned repo answers a bare `GET` for any token, so mere reachability proves
//! nothing; only the authenticated user's `push` permission proves the
//! invitation was accepted.

use crate::config::Config;
use crate::output;
use crate::output::OutputFormat;
use clap::Subcommand;
use eyre::{Context, Result};
use serde::Serialize;
use std::process::Stdio;
use tabled::Tabled;

/// Owned repos get `push` and nothing wider: the least that lets the bot push
/// a branch and open a PR (`pull`/`triage` cannot push), while `master`
/// protection and the no-self-approve boundary keep it from merging.
const COLLABORATOR_PERMISSION: &str = "push";

#[derive(Subcommand)]
pub enum GithubCommands {
    #[command(
        visible_alias = "i",
        about = "Invite the machine user to the owned-repo allowlist (as the owner)"
    )]
    Invite,
    #[command(
        visible_alias = "v",
        about = "Verify the machine user's token authenticates and reaches every allowlist repo"
    )]
    Verify {
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
    },
}

/// The result of one `gh` invocation, its success and streams separated so a
/// caller reads an outcome rather than parses rendered text.
#[derive(Debug, Clone)]
pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// The seam every `gh` call goes through, injected so the provisioning logic
/// is reachable from a test that never shells out (ADR-0047). `token` selects
/// the identity: `None` uses the operator's ambient `gh` auth (the owner),
/// `Some` runs as that token's account (the bot) via `GH_TOKEN`.
pub trait Gh {
    fn run(&self, args: &[&str], token: Option<&str>) -> Result<GhOutput>;
}

/// The production impl. Setting `GH_TOKEN` overrides the keyring auth, so a
/// bot-token call never acts as the owner.
pub struct LiveGh;

impl Gh for LiveGh {
    fn run(&self, args: &[&str], token: Option<&str>) -> Result<GhOutput> {
        let mut cmd = std::process::Command::new("gh");
        cmd.args(args).stdin(Stdio::null());
        if let Some(t) = token {
            cmd.env("GH_TOKEN", t);
        }
        let out = cmd
            .output()
            .wrap_err("failed to run `gh` — is the GitHub CLI installed and authenticated?")?;
        Ok(GhOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

/// The active `gh` account — the owner, read before any mutation so a bot
/// running its own provisioning is refused up front.
fn active_login(gh: &dyn Gh) -> Result<String> {
    let out = gh.run(&["api", "user", "--jq", ".login"], None)?;
    if !out.success {
        eyre::bail!(
            "could not read the active gh account (authenticate as the owner: `gh auth login`): {}",
            out.stderr
        );
    }
    Ok(out.stdout)
}

/// The account the stored token authenticates as. A hard failure here is
/// operational (expired or malformed token); a *different* login is a finding
/// the caller reports, not an error.
fn token_login(gh: &dyn Gh, token: &str) -> Result<String> {
    let out = gh.run(&["api", "user", "--jq", ".login"], Some(token))?;
    if !out.success {
        eyre::bail!(
            "the stored token could not authenticate against GitHub (expired or malformed?): {}",
            out.stderr
        );
    }
    Ok(out.stdout)
}

fn invite_collaborator(gh: &dyn Gh, repo: &str, bot: &str, permission: &str) -> Result<()> {
    let path = format!("repos/{repo}/collaborators/{bot}");
    let permission_field = format!("permission={permission}");
    let out = gh.run(
        &["api", "--method", "PUT", &path, "-f", &permission_field],
        None,
    )?;
    if !out.success {
        eyre::bail!(
            "could not invite {bot} to {repo} (do you hold admin on it? is the handle spelled right?): {}",
            out.stderr
        );
    }
    Ok(())
}

/// Whether the token's account holds push on the repo — the field that proves
/// an accepted invitation, since a public repo answers a bare `GET` regardless.
fn has_push(gh: &dyn Gh, token: &str, repo: &str) -> Result<bool> {
    let path = format!("repos/{repo}");
    let out = gh.run(&["api", &path, "--jq", ".permissions.push"], Some(token))?;
    Ok(out.success && out.stdout == "true")
}

/// The repos the bot has been invited to but not yet accepted, read from its
/// own pending invitations.
fn pending_invitations(gh: &dyn Gh, token: &str) -> Result<Vec<String>> {
    let out = gh.run(
        &[
            "api",
            "/user/repository_invitations",
            "--jq",
            ".[].repository.full_name",
        ],
        Some(token),
    )?;
    if !out.success {
        eyre::bail!(
            "could not list the machine user's pending repository invitations: {}",
            out.stderr
        );
    }
    Ok(out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// The machine account handle and its owned-repo allowlist, read from config.
/// A missing handle or empty allowlist is operational — nothing to act on.
fn bot_and_repos(config: &Config) -> Result<(String, Vec<String>)> {
    let bot = config
        .get("github_bot_login")
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| eyre::eyre!("github_bot_login is not set — the machine account handle"))?;
    let repos: Vec<String> = config
        .get("github_bot_repos")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if repos.is_empty() {
        eyre::bail!("github_bot_repos is empty — list at least one owned owner/repo");
    }
    Ok((bot, repos))
}

/// The whole invite phase, injected so a test drives it without a real `gh`:
/// refuse to act as the bot, then grant push on each owned repo.
fn invite_all(gh: &dyn Gh, bot: &str, repos: &[String], permission: &str) -> Result<()> {
    let active = active_login(gh)?;
    if active == bot {
        eyre::bail!(
            "the active gh account is the machine user ({active}) — invite must run as the owner, \
             who holds admin on the allowlist repos"
        );
    }
    for repo in repos {
        invite_collaborator(gh, repo, bot, permission)?;
        output::success(&format!("invited {bot} to {repo} ({permission})"));
    }
    Ok(())
}

/// Where one repo stands from the bot's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RepoState {
    /// Push confirmed — the invitation was accepted.
    Reachable,
    /// Invited, awaiting the bot's acceptance.
    Pending,
    /// No push and no pending invite — not provisioned.
    Unreachable,
}

impl RepoState {
    fn as_str(self) -> &'static str {
        match self {
            RepoState::Reachable => "reachable",
            RepoState::Pending => "pending",
            RepoState::Unreachable => "unreachable",
        }
    }
}

#[derive(Debug, Serialize)]
struct RepoAccess {
    repo: String,
    state: RepoState,
}

#[derive(Debug, Serialize)]
struct VerifyReport {
    bot_login: String,
    token_login: String,
    identity_ok: bool,
    repos: Vec<RepoAccess>,
    /// ADR-0044 discriminator: `verified` iff the token is the bot and every
    /// repo is reachable.
    outcome: &'static str,
}

impl VerifyReport {
    fn is_verified(&self) -> bool {
        self.identity_ok && self.repos.iter().all(|r| r.state == RepoState::Reachable)
    }
}

/// The whole verify phase, injected for the same reason as [`invite_all`]:
/// prove the token is the bot, then classify every allowlist repo.
fn verify_access(gh: &dyn Gh, bot: &str, repos: &[String], token: &str) -> Result<VerifyReport> {
    let login = token_login(gh, token)?;
    let identity_ok = login == bot;

    let pending = pending_invitations(gh, token)?;
    let mut accesses = Vec::with_capacity(repos.len());
    for repo in repos {
        let state = if has_push(gh, token, repo)? {
            RepoState::Reachable
        } else if pending.iter().any(|p| p == repo) {
            RepoState::Pending
        } else {
            RepoState::Unreachable
        };
        accesses.push(RepoAccess {
            repo: repo.clone(),
            state,
        });
    }

    let mut report = VerifyReport {
        bot_login: bot.to_string(),
        token_login: login,
        identity_ok,
        repos: accesses,
        outcome: "unverified",
    };
    if report.is_verified() {
        report.outcome = "verified";
    }
    Ok(report)
}

#[derive(Tabled)]
struct RepoAccessRow {
    #[tabled(rename = "REPO")]
    repo: String,
    #[tabled(rename = "STATE")]
    state: String,
}

pub fn run_github_invite() -> Result<()> {
    let config = Config::load()?;
    let (bot, repos) = bot_and_repos(&config)?;
    invite_all(&LiveGh, &bot, &repos, COLLABORATOR_PERMISSION)?;
    output::info("the machine user must now accept each invitation while logged in as itself");
    Ok(())
}

/// Returns the process exit code: 0 verified, 1 not verified. Operational
/// errors (missing config, unresolvable or unauthenticating token) propagate
/// as `Err` and exit non-zero without a report body.
pub fn run_github_verify(output_fmt: OutputFormat) -> Result<i32> {
    let config = Config::load()?;
    let (bot, repos) = bot_and_repos(&config)?;
    let token = config
        .get_resolved("github_bot_token")?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            eyre::eyre!(
                "github_bot_token is not set — mint the fine-grained PAT (see the docs \
                 checklist) and store it, e.g. github_bot_token = \"!pa show fleet/github-pat\""
            )
        })?;

    let report = verify_access(&LiveGh, &bot, &repos, &token)?;

    match output_fmt {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Human => {
            if report.identity_ok {
                output::success(&format!("token authenticates as {}", report.token_login));
            } else {
                output::warn(&format!(
                    "token authenticates as {}, not {}",
                    report.token_login, report.bot_login
                ));
            }
            let rows: Vec<RepoAccessRow> = report
                .repos
                .iter()
                .map(|a| RepoAccessRow {
                    repo: a.repo.clone(),
                    state: a.state.as_str().to_string(),
                })
                .collect();
            output::print_table(&rows);
        }
    }

    Ok(if report.is_verified() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct MockGh {
        active_login: String,
        token_login: String,
        invite_ok: bool,
        pending: Vec<String>,
        pushable: Vec<String>,
        calls: RefCell<Vec<String>>,
    }

    impl MockGh {
        fn new() -> Self {
            Self {
                active_login: "owner".to_string(),
                token_login: "fleet-bot".to_string(),
                invite_ok: true,
                pending: Vec::new(),
                pushable: Vec::new(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    fn ok(stdout: &str) -> Result<GhOutput> {
        Ok(GhOutput {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn fail() -> Result<GhOutput> {
        Ok(GhOutput {
            success: false,
            stdout: String::new(),
            stderr: "gh api failed".to_string(),
        })
    }

    impl Gh for MockGh {
        fn run(&self, args: &[&str], token: Option<&str>) -> Result<GhOutput> {
            self.calls.borrow_mut().push(args.join(" "));
            match args {
                ["api", "user", "--jq", ".login"] => ok(if token.is_some() {
                    &self.token_login
                } else {
                    &self.active_login
                }),
                ["api", "--method", "PUT", _, "-f", _] => {
                    if self.invite_ok {
                        ok("")
                    } else {
                        fail()
                    }
                }
                ["api", "/user/repository_invitations", "--jq", _] => ok(&self.pending.join("\n")),
                ["api", path, "--jq", ".permissions.push"] => {
                    let repo = path.strip_prefix("repos/").unwrap_or(path);
                    ok(if self.pushable.iter().any(|r| r == repo) {
                        "true"
                    } else {
                        "false"
                    })
                }
                _ => panic!("unexpected gh call: {}", args.join(" ")),
            }
        }
    }

    fn repos() -> Vec<String> {
        vec![
            "sripwoud/auberge".to_string(),
            "sripwoud/dotfiles".to_string(),
        ]
    }

    #[test]
    fn invite_grants_push_on_every_owned_repo() {
        let gh = MockGh::new();
        invite_all(&gh, "fleet-bot", &repos(), "push").unwrap();
        assert_eq!(
            gh.calls(),
            vec![
                "api user --jq .login".to_string(),
                "api --method PUT repos/sripwoud/auberge/collaborators/fleet-bot -f permission=push"
                    .to_string(),
                "api --method PUT repos/sripwoud/dotfiles/collaborators/fleet-bot -f permission=push"
                    .to_string(),
            ]
        );
    }

    /// The owner guard: a bot running its own provisioning defeats both the
    /// honest-review boundary and blast-radius containment.
    #[test]
    fn invite_refuses_to_run_as_the_bot() {
        let mut gh = MockGh::new();
        gh.active_login = "fleet-bot".to_string();
        let err = invite_all(&gh, "fleet-bot", &repos(), "push").unwrap_err();
        assert!(err.to_string().contains("must run as the owner"), "{err}");
        assert_eq!(gh.calls(), vec!["api user --jq .login".to_string()]);
    }

    #[test]
    fn a_failed_invitation_stops_the_run() {
        let mut gh = MockGh::new();
        gh.invite_ok = false;
        let err = invite_all(&gh, "fleet-bot", &repos(), "push").unwrap_err();
        assert!(err.to_string().contains("could not invite"), "{err}");
    }

    #[test]
    fn verify_is_verified_when_the_bot_reaches_every_repo() {
        let mut gh = MockGh::new();
        gh.pushable = repos();
        let report = verify_access(&gh, "fleet-bot", &repos(), "tok").unwrap();
        assert!(report.identity_ok);
        assert_eq!(report.outcome, "verified");
        assert!(report.repos.iter().all(|r| r.state == RepoState::Reachable));
        assert!(report.is_verified());
    }

    /// A token that is not the bot is a finding, not an error — the report
    /// still renders, the outcome is `unverified`.
    #[test]
    fn verify_flags_a_token_that_is_not_the_bot() {
        let mut gh = MockGh::new();
        gh.token_login = "someone-else".to_string();
        gh.pushable = repos();
        let report = verify_access(&gh, "fleet-bot", &repos(), "tok").unwrap();
        assert!(!report.identity_ok);
        assert_eq!(report.outcome, "unverified");
        assert!(!report.is_verified());
    }

    /// The gap Q3/Q8 kept verify around for: an invitation the bot has not
    /// accepted reads as pending, distinct from never-invited.
    #[test]
    fn verify_reports_an_unaccepted_invite_as_pending() {
        let mut gh = MockGh::new();
        gh.pushable = vec!["sripwoud/auberge".to_string()];
        gh.pending = vec!["sripwoud/dotfiles".to_string()];
        let report = verify_access(&gh, "fleet-bot", &repos(), "tok").unwrap();
        let dotfiles = report
            .repos
            .iter()
            .find(|r| r.repo == "sripwoud/dotfiles")
            .unwrap();
        assert_eq!(dotfiles.state, RepoState::Pending);
        assert_eq!(report.outcome, "unverified");
    }

    #[test]
    fn verify_reports_a_repo_with_neither_push_nor_invite_as_unreachable() {
        let gh = MockGh::new();
        let report = verify_access(&gh, "fleet-bot", &repos(), "tok").unwrap();
        assert!(
            report
                .repos
                .iter()
                .all(|r| r.state == RepoState::Unreachable)
        );
        assert_eq!(report.outcome, "unverified");
    }

    /// A public owned repo answers a bare GET for any token, so `verify` reads
    /// push permission — a token with only read is not a reachable repo.
    #[test]
    fn public_read_only_access_does_not_count_as_reachable() {
        let gh = MockGh::new();
        let report =
            verify_access(&gh, "fleet-bot", &["sripwoud/auberge".to_string()], "tok").unwrap();
        assert_eq!(report.repos[0].state, RepoState::Unreachable);
    }

    #[test]
    fn bot_and_repos_splits_the_space_separated_allowlist() {
        let config = Config::from_toml_str(
            "github_bot_login = \"fleet-bot\"\n\
             github_bot_repos = \"sripwoud/auberge sripwoud/dotfiles\"",
        )
        .unwrap();
        let (bot, repos) = bot_and_repos(&config).unwrap();
        assert_eq!(bot, "fleet-bot");
        assert_eq!(repos, vec!["sripwoud/auberge", "sripwoud/dotfiles"]);
    }

    #[test]
    fn bot_and_repos_rejects_an_empty_allowlist() {
        let config = Config::from_toml_str("github_bot_login = \"fleet-bot\"").unwrap();
        assert!(
            bot_and_repos(&config)
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
    }

    #[test]
    fn bot_and_repos_rejects_a_missing_login() {
        let config = Config::from_toml_str("github_bot_repos = \"sripwoud/auberge\"").unwrap();
        assert!(
            bot_and_repos(&config)
                .unwrap_err()
                .to_string()
                .contains("github_bot_login")
        );
    }
}
