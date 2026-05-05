use crate::ansible_assets::AnsibleAssets;
use crate::config::Config;
use crate::output;
use crate::prompt::{confirm, select_multi};
use crate::services::ansible_runner::{InventoryHost, run_playbook};
use crate::services::dependency_resolver::{
    PlaybookRun, get_app_names, resolve_tags_to_playbook_runs,
};
use crate::services::dns_verify::{
    AppVerifyConfig, HickoryLookup, app_verify_config, format_dns_error, verify_a_record,
};
use crate::services::inventory::{Host, select_or_arg};
use clap::Args;
use eyre::Result;

const ALL_ENTRY: &str = "[all]";

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
    select_or_arg(host_arg)
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

fn validate_apps(requested: &[String], available: &[String]) -> Result<()> {
    let unknown: Vec<&String> = requested
        .iter()
        .filter(|app| !available.contains(app))
        .collect();

    if !unknown.is_empty() {
        eyre::bail!(
            "Unknown app(s): {}. Available: {}",
            unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            available.join(", ")
        );
    }
    Ok(())
}

fn show_execution_plan(runs: &[PlaybookRun], host: &Host, check: bool) -> Result<()> {
    eprintln!();
    if check {
        output::info("Execution plan (DRY RUN):");
    } else {
        output::info("Execution plan:");
    }
    output::info(&format!(
        "  Host: {} ({})",
        host.name, host.vars.ansible_host
    ));
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

fn prepend_hardening(runs: Vec<PlaybookRun>) -> Result<Vec<PlaybookRun>> {
    let assets = AnsibleAssets::prepare()?;
    let hardening_path = assets.playbooks_dir().join("hardening.yml");
    let canonical = std::fs::canonicalize(&hardening_path).map_err(|e| {
        eyre::eyre!(
            "hardening playbook not found at {}: {}",
            hardening_path.display(),
            e
        )
    })?;

    let mut all_runs = vec![PlaybookRun {
        path: canonical,
        tags: vec![],
    }];
    all_runs.extend(runs);
    Ok(all_runs)
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

/// Run DNS verification checks for every app tag in `run` (only for apps.yml runs).
/// Failures are reported as errors; mismatches / NXDOMAIN / lookup errors each
/// produce an actionable diagnostic naming the FQDN, resolver, and mismatch.
fn run_dns_checks_for_run(
    run: &PlaybookRun,
    config: &Config,
    host: &Host,
    verify_public: bool,
) -> Result<()> {
    if !run.is_apps() || run.tags.is_empty() {
        return Ok(());
    }

    let domain = config.domain();
    let ansible_host = &host.vars.ansible_host;
    let mut errors: Vec<String> = Vec::new();

    for tag in &run.tags {
        let Some(vc): Option<AppVerifyConfig> =
            app_verify_config(tag, &domain, ansible_host, config, verify_public)
        else {
            continue;
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
        validate_apps(&cmd.apps, &available_apps)?;
        cmd.apps.clone()
    };

    let host = select_host(cmd.host)?;

    let (resolved_runs, unknown_tags) = resolve_tags_to_playbook_runs(&apps)?;

    if !unknown_tags.is_empty() {
        output::warn(&format!("Unknown tags: {}", unknown_tags.join(", ")));
    }

    if resolved_runs.is_empty() {
        eyre::bail!("No playbook runs resolved for apps: {}", apps.join(", "));
    }

    let runs = prepend_hardening(resolved_runs)?;

    // Validate config and build preflights for all runs upfront so we fail
    // fast before executing any playbook.
    let config = Config::load()?;
    let preflights: Vec<_> = runs
        .iter()
        .map(|run| {
            let name = run.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let tags = if run.tags.is_empty() {
                None
            } else {
                Some(run.tags.as_slice())
            };
            config.preflight_for(name, tags)
        })
        .collect::<Result<_>>()?;

    show_execution_plan(&runs, &host, cmd.check)?;
    warn_apps_prerequisites(&runs);
    confirm_deploy(cmd.force)?;

    let inventory_host = InventoryHost {
        name: host.name.clone(),
        address: host.vars.ansible_host.clone(),
        port: host.vars.ansible_port,
        user: host.vars.bootstrap_user.clone(),
    };

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

        let mut progress = crate::services::progress::TerminalProgress::new("");
        let result = run_playbook(
            preflight,
            &run.path,
            &inventory_host,
            cmd.check,
            run_tags,
            None,
            None,
            false,
            false,
            &mut progress,
        )?;

        if !result.success {
            if result.last_output.is_empty() {
                eyre::bail!(
                    "{} failed with exit code {}",
                    playbook_name,
                    result.exit_code
                );
            } else {
                eyre::bail!(
                    "{} failed with exit code {}:\n{}",
                    playbook_name,
                    result.exit_code,
                    result.last_output.trim()
                );
            }
        }

        output::success(&format!("{} completed successfully", playbook_name));

        if !cmd.check {
            run_dns_checks_for_run(run, &config, &host, cmd.verify_public_dns)?;
        }
    }

    output::success("Deployment completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_apps_all_valid() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        assert!(validate_apps(&["paperless".to_string()], &available).is_ok());
    }

    #[test]
    fn test_validate_apps_unknown() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        let result = validate_apps(&["nonexistent".to_string()], &available);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent"));
    }

    #[test]
    fn test_validate_apps_mixed_valid_and_unknown() {
        let available = vec!["paperless".to_string(), "freshrss".to_string()];
        let result = validate_apps(&["paperless".to_string(), "badapp".to_string()], &available);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("badapp"));
    }

    #[test]
    fn test_validate_apps_empty_requested() {
        let available = vec!["paperless".to_string()];
        assert!(validate_apps(&[], &available).is_ok());
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
        assert!(apps.contains(&"blocky".to_string()));
        assert!(!apps.contains(&"caddy".to_string()));
    }
}
