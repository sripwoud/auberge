use crate::models::inventory::Host;
use crate::models::playbook::Playbook;
use crate::output;
use crate::playbooks::PlaybookManager;
use crate::selector::{select_item, select_multi};
use crate::services::ansible_runner::{InventoryHost, required_config_keys, run_playbook};
use crate::services::dependency_resolver::{
    PlaybookRun, get_app_names, resolve_tags_to_playbook_runs,
};
use crate::services::inventory::get_hosts;
use crate::user_config::UserConfig;
use clap::Args;
use eyre::Result;
use std::io::{self, Write};

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
}

fn select_host(host_arg: Option<String>) -> Result<Host> {
    match host_arg {
        Some(name) => crate::services::inventory::get_host(&name, None),
        None => {
            let hosts = get_hosts(None, None)?;
            select_item(
                &hosts,
                |h: &Host| {
                    format!(
                        "{} ({}:{})",
                        h.name, h.vars.ansible_host, h.vars.ansible_port
                    )
                },
                "Select host",
            )?
            .ok_or_else(|| eyre::eyre!("No host selected"))
        }
    }
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

fn validate_config_for_deploy() -> Result<UserConfig> {
    let config = UserConfig::load()?;
    let mut all_keys = Vec::new();
    for playbook in ["hardening.yml", "infrastructure.yml", "apps.yml"] {
        for key in required_config_keys(playbook) {
            if !all_keys.contains(&key) {
                all_keys.push(key);
            }
        }
    }

    let missing = config.validate_required(&all_keys);
    if !missing.is_empty() {
        output::error("Missing required config values:");
        for key in &missing {
            output::error(&format!(
                "  '{}' is required. Run: auberge config set {} <VALUE>",
                key, key
            ));
        }
        eyre::bail!(
            "{} required config value(s) missing in config.toml",
            missing.len()
        );
    }
    Ok(config)
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
    let playbooks_dir = PlaybookManager::get_playbooks_dir()?;
    let hardening_path = playbooks_dir.join("hardening.yml");
    let canonical = std::fs::canonicalize(&hardening_path)
        .map_err(|_| eyre::eyre!("hardening.yml not found in {}", playbooks_dir.display()))?;

    let mut all_runs = vec![PlaybookRun {
        path: canonical,
        tags: vec![],
    }];
    all_runs.extend(runs);
    Ok(all_runs)
}

fn confirm_deploy(force: bool) -> Result<()> {
    if force {
        return Ok(());
    }

    print!("Proceed with deployment? [y/N]: ");
    io::stdout().flush()?;
    let mut response = String::new();
    io::stdin().read_line(&mut response)?;

    if !response.trim().eq_ignore_ascii_case("y") {
        eprintln!("Aborted.");
        std::process::exit(1);
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

    validate_config_for_deploy()?;

    let (resolved_runs, unknown_tags) = resolve_tags_to_playbook_runs(&apps)?;

    if !unknown_tags.is_empty() {
        output::warn(&format!("Unknown tags: {}", unknown_tags.join(", ")));
    }

    if resolved_runs.is_empty() {
        eyre::bail!("No playbook runs resolved for apps: {}", apps.join(", "));
    }

    let runs = prepend_hardening(resolved_runs)?;

    show_execution_plan(&runs, &host, cmd.check)?;
    confirm_deploy(cmd.force)?;

    let inventory_host = InventoryHost {
        name: host.name.clone(),
        address: host.vars.ansible_host.clone(),
        port: host.vars.ansible_port,
        user: host.vars.bootstrap_user.clone(),
    };

    for run in &runs {
        let playbook = Playbook::from_path(run.path.clone());
        let run_tags = if run.tags.is_empty() {
            None
        } else {
            Some(run.tags.as_slice())
        };

        output::info(&format!(
            "Running {} on {}{}",
            playbook.name,
            host.name,
            run_tags.map_or(String::new(), |t| format!(" (tags: {})", t.join(", ")))
        ));

        let result = run_playbook(
            &playbook.path,
            &inventory_host,
            cmd.check,
            run_tags,
            None,
            None,
            false,
            false,
        )?;

        if !result.success {
            eyre::bail!(
                "{} failed with exit code {}",
                playbook.name,
                result.exit_code
            );
        }

        output::success(&format!("{} completed successfully", playbook.name));
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
        let playbooks_dir = PlaybookManager::get_playbooks_dir().unwrap();
        let apps_path = std::fs::canonicalize(playbooks_dir.join("apps.yml")).unwrap();
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
