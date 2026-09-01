use crate::ansible_assets::AnsibleAssets;
use crate::commands::headscale::{INJECTED_AUTHKEY, preauth_key_for_plan};
use crate::config::Config;
use crate::hosts::{HOST_FLAG, HostManager};
use crate::output;
use crate::playbook_meta::{app_memory_vars, app_version_vars};
use crate::prompt::{confirm, select_multi};
use crate::services::ansible_runner::{InventoryHost, run_playbook};
use crate::services::dependency_resolver::{
    HARDENING_PLAYBOOK, INFRASTRUCTURE_PLAYBOOK, PlaybookRun, find_standalone_playbook,
    get_app_names, get_infrastructure_role_names, parse_roster, resolve_tags_to_playbook_runs,
    standalone_playbook_names,
};
use crate::services::dns::app_parent_domain;
use crate::services::dns_verify::{
    HickoryLookup, TailnetResolver, app_verify_config, format_dns_error, verify_a_record,
};
use crate::services::inventory::{Host, hosts_ignoreip_var, select_or_arg};
use clap::Args;
use eyre::Result;

const ALL_ENTRY: &str = "[all]";

/// The standalone playbooks `deploy` refuses, each with the reason it is a
/// lifecycle operation rather than a convergence toward a declared state.
///
/// Everything else standalone is deployable, read off the tree, so a new
/// playbook is reachable from `deploy` the day it lands.
/// `tests/deployable_playbooks.rs` holds the tree to this list, so one that
/// should not be reachable fails the build until it says why.
pub const NOT_DEPLOYABLE: &[(&str, &str)] = &[
    (
        "bootstrap",
        "runs as root over port 22 before the ansible user exists, and needs \
         the provider-firewall confirmation and the port-22 transport that \
         only `auberge ansible run` carries",
    ),
    (
        "remove-radicale",
        "tears an App down. `deploy` converges a Host toward a declared state \
         and prepends Substrate to do it, which is the wrong two layers to run \
         ahead of a removal",
    ),
];

#[derive(Args)]
pub struct DeployCmd {
    #[arg(help = "App(s) to deploy (e.g. paperless freshrss)")]
    pub apps: Vec<String>,
    #[arg(short = 'H', long, help = "Target host")]
    pub host: Option<String>,
    #[arg(short = 'C', long, help = "Dry-run mode (ansible check mode)")]
    pub check: bool,
    #[arg(long, help = "Deploy all apps", conflicts_with = "apps")]
    pub all: bool,
    #[arg(short = 'f', long, help = "Skip confirmation prompt")]
    pub force: bool,
    #[arg(
        long,
        help = "Verify public DNS after each app's playbook run (queries 1.1.1.1)"
    )]
    pub verify_public_dns: bool,
}

fn select_host(host_arg: Option<String>) -> Result<Host> {
    select_or_arg(host_arg, HOST_FLAG)
}

fn select_apps(available: &[String]) -> Result<Vec<String>> {
    if available.len() == 1 {
        return Ok(available.to_vec());
    }

    let mut items: Vec<String> = vec![ALL_ENTRY.to_string()];
    items.extend(available.iter().cloned());

    let selected = select_multi(
        &items,
        "Select app(s) to deploy (tab to toggle, enter to confirm)",
    )
    .ok_or_else(|| eyre::eyre!("No apps selected"))?;

    if selected.iter().any(|s| s == ALL_ENTRY) {
        return Ok(available.to_vec());
    }

    Ok(selected)
}

/// The standalone playbooks this build will deploy: every one in the tree that
/// [`NOT_DEPLOYABLE`] does not name.
fn deployable_playbooks() -> Result<Vec<String>> {
    Ok(standalone_playbook_names()?
        .into_iter()
        .filter(|name| !NOT_DEPLOYABLE.iter().any(|(excluded, _)| excluded == name))
        .collect())
}

fn validate_apps(
    requested: &[String],
    available: &[String],
    playbooks: &[String],
    infra_roles: &[String],
) -> Result<()> {
    let (infra, unknown): (Vec<&String>, Vec<&String>) = requested
        .iter()
        .filter(|app| !available.contains(app) && !playbooks.contains(app))
        .partition(|app| infra_roles.contains(app));

    if infra.is_empty() && unknown.is_empty() {
        return Ok(());
    }

    let mut messages: Vec<String> = infra
        .iter()
        .map(|role| {
            format!(
                "`{role}` is declared in ansible/playbooks/infrastructure.yml, not apps.yml.\n\
                 It deploys on every `auberge deploy <app>` run.\n\
                 To deploy it alone: auberge ansible run -t {role}"
            )
        })
        .collect();

    if !unknown.is_empty() {
        let mut offered: Vec<&str> = available
            .iter()
            .chain(playbooks.iter())
            .map(String::as_str)
            .collect();
        offered.sort_unstable();
        offered.dedup();
        messages.push(format!(
            "Unknown app(s): {}. Available: {}",
            unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            offered.join(", ")
        ));
    }

    eyre::bail!("{}", messages.join("\n"));
}

fn show_execution_plan(runs: &[PlaybookRun], host: &Host, check: bool) -> Result<()> {
    eprintln!();
    if check {
        output::info("Execution plan (DRY RUN):");
    } else {
        output::info("Execution plan:");
    }
    output::info(&format!("  Host: {} ({})", host.name, host.connect_address));
    for run in runs {
        let name = run
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        if run.tags.is_empty() {
            output::info(&format!("  → {}", name));
        } else {
            output::info(&format!("  → {} (tags: {})", name, run.tags.join(", ")));
        }
    }
    eprintln!();
    Ok(())
}

/// An untagged run of one of the tree's own playbooks, by file name.
fn playbook_run(filename: &str) -> Result<PlaybookRun> {
    let assets = AnsibleAssets::prepare()?;
    let path = assets.playbooks_dir().join(filename);
    let canonical = std::fs::canonicalize(&path)
        .map_err(|e| eyre::eyre!("{filename} not found at {}: {}", path.display(), e))?;
    Ok(PlaybookRun {
        path: canonical,
        tags: Vec::new(),
    })
}

fn prepend_hardening(runs: Vec<PlaybookRun>) -> Result<Vec<PlaybookRun>> {
    let mut all_runs = vec![playbook_run(HARDENING_PLAYBOOK)?];
    all_runs.extend(runs);
    Ok(all_runs)
}

/// The requested names split into the two routes `deploy` has: the standalone
/// playbooks to run whole, and the tags to resolve against the roster.
///
/// A name the roster holds keeps going through the roster even when a
/// standalone playbook of that name exists — that is where calibre (#559) and
/// immich (#580) deploy from, and it is the route that runs the DNS check.
fn split_routes(requested: &[String], roster: &[String]) -> (Vec<String>, Vec<String>) {
    requested
        .iter()
        .cloned()
        .partition(|name| !roster.contains(name))
}

/// The runs a plan's standalone playbook names add, in the order they were
/// asked for.
fn standalone_runs(names: &[String]) -> Result<Vec<PlaybookRun>> {
    let mut runs = Vec::new();
    for name in names {
        if let Some((_, why)) = NOT_DEPLOYABLE.iter().find(|(excluded, _)| excluded == name) {
            eyre::bail!("`{name}` is not a deploy target: {why}");
        }
        // Validation already resolved this name, so a miss here is the tree
        // moving under the run. Skipping it would deploy nothing and report
        // success, which is the one outcome worse than an error.
        let Some(path) = find_standalone_playbook(name)? else {
            eyre::bail!(
                "`{name}` validated as a playbook but no longer resolves to one; \
                 the assets tree changed mid-run"
            );
        };
        runs.push(PlaybookRun {
            path,
            tags: Vec::new(),
        });
    }
    Ok(runs)
}

/// The tag-resolved runs with the standalone ones appended, and Substrate
/// ahead of both.
///
/// A standalone playbook pulls `infrastructure.yml` in for the same reason an
/// apps.yml tag does: caddy, tailscale and the shell are what every App on the
/// Host stands on, and the agent tier's dashboard is unreachable until a Caddy
/// holding a certificate for its zone is in front of it (ADR-0072). `deploy`
/// is the verb that brings a Host to the state one App needs; `auberge ansible
/// run` stays the surgical one and runs the playbook alone.
fn with_substrate(
    mut runs: Vec<PlaybookRun>,
    standalone: Vec<PlaybookRun>,
) -> Result<Vec<PlaybookRun>> {
    if standalone.is_empty() {
        return Ok(runs);
    }
    if !runs.iter().any(PlaybookRun::is_infrastructure) {
        // Front, not "after any hardening run": the caller hands this the
        // tag-resolved runs, which hold only infrastructure and apps —
        // hardening is prepended after, by `prepend_hardening`.
        runs.insert(0, playbook_run(INFRASTRUCTURE_PLAYBOOK)?);
    }
    runs.extend(standalone);
    Ok(runs)
}

fn warn_apps_prerequisites(runs: &[PlaybookRun]) {
    if runs.iter().any(PlaybookRun::is_apps) {
        output::warn(
            "Ensure Cloudflare API token is configured and provider firewall allows port 853 (DNS-over-TLS)",
        );
    }
}

fn confirm_deploy(force: bool) -> Result<()> {
    if !confirm("Proceed with deployment?", force) {
        eprintln!("Aborted.");
        std::process::exit(1);
    }
    Ok(())
}

/// The Apps a run publishes names for: its tags where it has them, and its
/// roster where it does not.
///
/// A composition carries no tags — `ruche.yml` runs whole — so reading only
/// `run.tags` would skip the check on precisely the run ADR-0071 was waiting
/// for, and `auberge deploy ruche` would report success without ever asking
/// whether `essaim` resolves. The roster is the same source `units_for_run`
/// expands an untagged run through, so the two agree about what a tagless run
/// deploys.
///
/// Substrate runs are excluded by name rather than by shape: hardening and
/// infrastructure have rosters too, and neither publishes an App's name.
fn published_apps(run: &PlaybookRun) -> Result<Vec<String>> {
    if run.is_hardening() || run.is_infrastructure() {
        return Ok(Vec::new());
    }
    if !run.tags.is_empty() {
        return Ok(run.tags.clone());
    }
    if run.is_apps() {
        // An untagged apps.yml run is the whole roster, guards and all, and a
        // guard is exactly the case whose name may belong to another Host.
        return Ok(Vec::new());
    }
    Ok(parse_roster(&run.path)?
        .into_iter()
        .map(|role| role.name)
        .collect())
}

/// Run DNS verification checks for every App a run publishes a name for.
/// Failures are reported as errors; mismatches / NXDOMAIN / lookup errors each
/// produce an actionable diagnostic naming the FQDN, resolver, and mismatch.
fn run_dns_checks_for_run(
    run: &PlaybookRun,
    config: &Config,
    host: &Host,
    verify_public: bool,
    tailnet_resolver: &TailnetResolver,
) -> Result<()> {
    let apps = published_apps(run)?;
    if apps.is_empty() {
        return Ok(());
    }

    let playbooks_dir = run.path.parent().unwrap_or(&run.path).to_path_buf();
    let public_address = &host.vars.public_address;
    let mut errors: Vec<String> = Vec::new();

    for tag in &apps {
        // Per App, not per run: the agent tier's Apps compose against their own
        // zone (ADR-0068), so the run cannot resolve one domain for all of them.
        let domain = app_parent_domain(&playbooks_dir, tag, config, Some(&host.name));
        let vc = match app_verify_config(
            tag,
            &domain,
            public_address,
            config,
            Some(&host.name),
            verify_public,
            tailnet_resolver,
        ) {
            Ok(Some(vc)) => vc,
            Ok(None) => continue,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };

        let kind = if vc.is_tailnet() { "tailnet" } else { "public" };
        output::info(&format!(
            "DNS check ({kind}): {} → {} via {}",
            tag, vc.fqdn, vc.resolver_ip
        ));

        let lookup = match HickoryLookup::new(&vc.resolver_ip) {
            Ok(l) => l,
            Err(e) => {
                errors.push(format!(
                    "Failed to build resolver for {} (resolver {}): {}",
                    vc.fqdn, vc.resolver_ip, e
                ));
                continue;
            }
        };
        match verify_a_record(&lookup, &vc.fqdn, &vc.expected_ip) {
            Ok(None) => {
                output::success(&format!("DNS OK: {} → {}", vc.fqdn, vc.expected_ip));
            }
            Ok(Some(failure)) => {
                errors.push(format_dns_error(
                    &vc.fqdn,
                    &vc.resolver_ip,
                    &vc.expected_ip,
                    &failure,
                ));
            }
            Err(e) => {
                errors.push(format!(
                    "DNS lookup error for {} (resolver {}): {}",
                    vc.fqdn, vc.resolver_ip, e
                ));
            }
        }
    }

    if !errors.is_empty() {
        eyre::bail!("DNS verification failed:\n{}", errors.join("\n"));
    }

    Ok(())
}

pub fn run_deploy(cmd: DeployCmd) -> Result<()> {
    let available_apps = get_app_names()?;
    if available_apps.is_empty() {
        eyre::bail!("No apps found in apps.yml");
    }

    let apps = if cmd.all {
        available_apps.clone()
    } else if cmd.apps.is_empty() {
        select_apps(&available_apps)?
    } else {
        validate_apps(
            &cmd.apps,
            &available_apps,
            &deployable_playbooks()?,
            &get_infrastructure_role_names()?,
        )?;
        cmd.apps.clone()
    };

    let host = select_host(cmd.host)?;

    let (playbooks, tags) = split_routes(&apps, &available_apps);

    let (resolved_runs, unknown_tags) = resolve_tags_to_playbook_runs(&tags)?;

    if !unknown_tags.is_empty() {
        output::warn(&format!("Unknown tags: {}", unknown_tags.join(", ")));
    }

    let standalone = standalone_runs(&playbooks)?;
    if resolved_runs.is_empty() && standalone.is_empty() {
        eyre::bail!("No playbook runs resolved for apps: {}", apps.join(", "));
    }

    let runs = prepend_hardening(with_substrate(resolved_runs, standalone)?)?;

    // Validate config and build preflights for all runs upfront so we fail
    // fast before executing any playbook.
    let config = Config::load()?;
    let assets = AnsibleAssets::prepare()?;
    let preflights: Vec<_> = runs
        .iter()
        .map(|run| {
            let name = run.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let tags = if run.tags.is_empty() {
                None
            } else {
                Some(run.tags.as_slice())
            };
            crate::services::required_keys::preflight_for(
                &config,
                assets.ansible_dir(),
                name,
                tags,
                &host.name,
            )
        })
        .collect::<Result<_>>()?;

    show_execution_plan(&runs, &host, cmd.check)?;
    warn_apps_prerequisites(&runs);
    confirm_deploy(cmd.force)?;

    let inventory_host = InventoryHost {
        name: host.name.clone(),
        route: host.route(),
        groups: host.groups.clone(),
    };

    let tailnet_resolver = TailnetResolver::locate(&HostManager::load_hosts()?, &config);

    let playbooks_dir = assets.playbooks_dir();
    let app_versions = app_version_vars(&playbooks_dir)?;
    let memory_budgets = app_memory_vars(&playbooks_dir)?;
    let hosts_ignoreip = hosts_ignoreip_var()?;
    let extra_vars: Vec<(&str, &str)> = app_versions
        .iter()
        .chain(memory_budgets.iter())
        .chain(std::iter::once(&hosts_ignoreip))
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();

    for (run, preflight) in runs.iter().zip(preflights.iter()) {
        let playbook_name = run
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let run_tags = if run.tags.is_empty() {
            None
        } else {
            Some(run.tags.as_slice())
        };

        output::info(&format!(
            "Running {} on {}{}",
            playbook_name,
            host.name,
            run_tags.map_or(String::new(), |t| format!(" (tags: {})", t.join(", ")))
        ));

        // Minted per run, immediately before the run that consumes it: the key
        // has a TTL, and a plan's earlier playbooks would otherwise burn it.
        let preauth_key = preauth_key_for_plan(
            assets.ansible_dir(),
            &[(
                run.path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                run_tags,
            )],
            &host.name,
            cmd.check,
        )?;
        let mut run_extra_vars = extra_vars.clone();
        if let Some(key) = &preauth_key {
            run_extra_vars.push((INJECTED_AUTHKEY, key));
        }

        let mut progress = crate::services::progress::TerminalProgress::new("");
        let run_started = std::time::Instant::now();
        let result = run_playbook(
            preflight,
            &run.path,
            &inventory_host,
            cmd.check,
            run_tags,
            None,
            Some(&run_extra_vars),
            false,
            false,
            false,
            &mut progress,
        )?;

        if !result.success {
            let mut failure = if result.last_output.is_empty() {
                format!(
                    "{} failed with exit code {}",
                    playbook_name, result.exit_code
                )
            } else {
                format!(
                    "{} failed with exit code {}:\n{}",
                    playbook_name,
                    result.exit_code,
                    result.last_output.trim()
                )
            };
            // Check mode changes no unit, so there is no state to read out.
            if !cmd.check
                && let Some(report) = crate::services::unit_state::deploy_failure_unit_report(
                    run,
                    &host.name,
                    run_started.elapsed(),
                )
            {
                failure.push_str("\n\n");
                failure.push_str(&report);
            }
            eyre::bail!(failure);
        }

        output::success(&format!("{} completed successfully", playbook_name));

        if !cmd.check {
            run_dns_checks_for_run(
                run,
                &config,
                &host,
                cmd.verify_public_dns,
                &tailnet_resolver,
            )?;
        }
    }

    output::success("Deployment completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infra_roles() -> Vec<String> {
        vec![
            "caddy".to_string(),
            "blocky".to_string(),
            "headscale".to_string(),
        ]
    }

    fn playbooks() -> Vec<String> {
        vec!["ruche".to_string(), "memsearch".to_string()]
    }

    #[test]
    fn test_validate_apps_all_valid() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        assert!(
            validate_apps(
                &["paperless".to_string()],
                &available,
                &playbooks(),
                &infra_roles(),
            )
            .is_ok()
        );
    }

    #[test]
    fn test_validate_apps_unknown() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        let result = validate_apps(
            &["nonexistent".to_string()],
            &available,
            &playbooks(),
            &infra_roles(),
        );
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unknown app(s): nonexistent. Available: freshrss, memsearch, paperless, ruche"
        );
    }

    #[test]
    fn test_validate_apps_mixed_valid_and_unknown() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        let result = validate_apps(
            &["paperless".to_string(), "badapp".to_string()],
            &available,
            &playbooks(),
            &infra_roles(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("badapp"));
    }

    #[test]
    fn test_validate_apps_empty_requested() {
        let available = vec!["paperless".to_string()];
        assert!(validate_apps(&[], &available, &playbooks(), &infra_roles()).is_ok());
    }

    #[test]
    fn test_validate_apps_infra_role_points_to_ansible_run() {
        let available = vec!["paperless".to_string()];
        for role in ["blocky", "caddy", "headscale"] {
            let result = validate_apps(
                &[role.to_string()],
                &available,
                &playbooks(),
                &infra_roles(),
            );
            let message = result.unwrap_err().to_string();
            assert!(message.contains("ansible/playbooks/infrastructure.yml"));
            assert!(message.contains(&format!("auberge ansible run -t {role}")));
            assert!(!message.contains("Unknown app(s)"));
        }
    }

    #[test]
    fn test_validate_apps_infra_and_unknown_reported_distinctly() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        let result = validate_apps(
            &["blocky".to_string(), "nonexistent".to_string()],
            &available,
            &playbooks(),
            &infra_roles(),
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("auberge ansible run -t blocky"));
        assert!(message.contains(
            "Unknown app(s): nonexistent. Available: freshrss, memsearch, paperless, ruche"
        ));
    }

    #[test]
    fn test_validate_apps_infra_roles_from_playbook() {
        let available = get_app_names().unwrap();
        let infra = get_infrastructure_role_names().unwrap();
        let result = validate_apps(
            &["blocky".to_string()],
            &available,
            &deployable_playbooks().unwrap(),
            &infra,
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("auberge ansible run -t blocky"));
    }

    fn run_for(filename: &str, tags: &[&str]) -> PlaybookRun {
        let assets = AnsibleAssets::prepare().unwrap();
        let path = std::fs::canonicalize(assets.playbooks_dir().join(filename)).unwrap();
        PlaybookRun {
            path,
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    fn names(runs: &[PlaybookRun]) -> Vec<String> {
        runs.iter()
            .map(|run| run.path.file_name().unwrap().to_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn test_deployable_playbooks_offers_the_agent_tier_and_refuses_bootstrap() {
        let offered = deployable_playbooks().unwrap();
        assert!(offered.contains(&"ruche".to_string()), "got: {offered:?}");
        assert!(offered.contains(&"aoe".to_string()), "got: {offered:?}");
        assert!(
            !offered.contains(&"bootstrap".to_string()),
            "bootstrap must not be deployable: {offered:?}"
        );
    }

    #[test]
    fn test_split_routes_keeps_a_roster_name_on_the_roster() {
        // calibre is both; the roster route is the one that runs the DNS check.
        let roster = vec!["calibre".to_string(), "paperless".to_string()];
        let (playbooks, tags) = split_routes(
            &[
                "calibre".to_string(),
                "ruche".to_string(),
                "paperless".to_string(),
            ],
            &roster,
        );
        assert_eq!(playbooks, vec!["ruche".to_string()]);
        assert_eq!(tags, vec!["calibre".to_string(), "paperless".to_string()]);
    }

    #[test]
    fn test_with_substrate_puts_infrastructure_ahead_of_a_standalone_run() {
        let plan = with_substrate(Vec::new(), vec![run_for("ruche.yml", &[])]).unwrap();
        assert_eq!(names(&plan), vec!["infrastructure.yml", "ruche.yml"]);
    }

    #[test]
    fn test_with_substrate_does_not_add_a_second_infrastructure_run() {
        // The apps route already pulled it in; a duplicate would re-run caddy
        // and tailscale on a Host that just converged them.
        let plan = with_substrate(
            vec![
                run_for("infrastructure.yml", &[]),
                run_for("apps.yml", &["paperless"]),
            ],
            vec![run_for("ruche.yml", &[])],
        )
        .unwrap();
        assert_eq!(
            names(&plan),
            vec!["infrastructure.yml", "apps.yml", "ruche.yml"]
        );
    }

    #[test]
    fn test_a_composition_plan_is_hardening_then_substrate_then_the_playbook() {
        // The two assemblers in the order `run_deploy` calls them, so the
        // plan an operator sees for `auberge deploy ruche -H ruche` is
        // asserted whole rather than in halves.
        let plan =
            prepend_hardening(with_substrate(Vec::new(), vec![run_for("ruche.yml", &[])]).unwrap())
                .unwrap();
        assert_eq!(
            names(&plan),
            vec!["hardening.yml", "infrastructure.yml", "ruche.yml"]
        );
    }

    #[test]
    fn test_published_apps_reads_a_compositions_roster() {
        // `ruche.yml` carries no tags, so the DNS check has only the roster to
        // go on — and aoe is the App on it that publishes a name (ADR-0071).
        let apps = published_apps(&run_for("ruche.yml", &[])).unwrap();
        assert!(apps.contains(&"aoe".to_string()), "got: {apps:?}");
    }

    #[test]
    fn test_published_apps_is_empty_for_substrate_runs() {
        for filename in ["hardening.yml", "infrastructure.yml"] {
            let apps = published_apps(&run_for(filename, &[])).unwrap();
            assert!(apps.is_empty(), "{filename} published {apps:?}");
        }
    }

    #[test]
    fn test_published_apps_ignores_an_untagged_apps_roster() {
        // Its roster holds guarded roles whose names belong to another Host;
        // checking them here would fail a deploy on a name it never published.
        let apps = published_apps(&run_for("apps.yml", &[])).unwrap();
        assert!(apps.is_empty(), "got: {apps:?}");
    }

    #[test]
    fn test_standalone_runs_refuses_a_playbook_declared_off_the_path() {
        let err = standalone_runs(&["bootstrap".to_string()]).unwrap_err();
        assert!(
            err.to_string().contains("not a deploy target"),
            "got: {err}"
        );
    }

    #[test]
    fn test_with_substrate_leaves_a_plan_with_no_standalone_run_alone() {
        let plan = with_substrate(vec![run_for("apps.yml", &["paperless"])], Vec::new()).unwrap();
        assert_eq!(names(&plan), vec!["apps.yml"]);
    }

    #[test]
    fn test_prepend_hardening() {
        let assets = AnsibleAssets::prepare().unwrap();
        let apps_path = std::fs::canonicalize(assets.playbooks_dir().join("apps.yml")).unwrap();
        let runs = vec![PlaybookRun {
            path: apps_path.clone(),
            tags: vec!["paperless".to_string()],
        }];

        let result = prepend_hardening(runs).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].path.file_name().unwrap().to_str().unwrap(),
            "hardening.yml"
        );
        assert!(result[0].tags.is_empty());
        assert_eq!(
            result[1].path.file_name().unwrap().to_str().unwrap(),
            "apps.yml"
        );
    }

    #[test]
    fn test_get_app_names_returns_roles() {
        let apps = get_app_names().unwrap();
        assert!(apps.contains(&"paperless".to_string()));
        assert!(apps.contains(&"baikal".to_string()));
        assert!(apps.contains(&"freshrss".to_string()));
        assert!(!apps.contains(&"caddy".to_string()));
        assert!(!apps.contains(&"blocky".to_string()));
    }
}
