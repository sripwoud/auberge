use crate::config::{Config, Preflight};
use crate::hosts::{HOST_FLAG, HOST_POSITIONAL};
use crate::output;
use crate::playbook_meta::{app_memory_vars, app_version_vars};
use crate::prompt::{Choice, select_item};
use crate::services::ansible_runner::{InventoryHost, run_bootstrap, run_playbook};
use crate::services::dependency_resolver::{
    find_standalone_playbook, resolve_tags_to_playbook_runs,
};
use crate::services::inventory::{Host, get_playbooks, hosts_ignoreip_var, select_or_arg};
use clap::Subcommand;
use eyre::{Result, WrapErr};
use regex::Regex;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum AnsibleCommands {
    #[command(
        visible_alias = "r",
        about = "Run a playbook against a host, resolving tags to dependencies"
    )]
    Run {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            help = "Playbook path (auto-resolved from tags if omitted)"
        )]
        playbook: Option<PathBuf>,
        #[arg(short = 'C', long, help = "Run in check mode (dry run)")]
        check: bool,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Comma-separated tags to run (auto-deploys infra dependencies; a standalone playbook name runs that playbook)"
        )]
        tags: Option<Vec<String>>,
        #[arg(long, value_delimiter = ',', help = "Skip tasks with these tags")]
        skip_tags: Option<Vec<String>>,
        #[arg(long, help = "Bootstrap user (overrides inventory setting)")]
        user: Option<String>,
        #[arg(long, help = "Prompt for SSH password (needed for initial bootstrap)")]
        ask_pass: bool,
        #[arg(
            short = 'f',
            long,
            help = "Skip confirmation prompts (for CI/CD automation)"
        )]
        force: bool,
    },
    #[command(
        visible_alias = "b",
        about = "Bootstrap a new host for ansible management"
    )]
    Bootstrap {
        #[arg(help = "Host name (omit to be prompted)")]
        host: Option<String>,
        #[arg(long, default_value = "22", help = "SSH port for initial connection")]
        port: u16,
        #[arg(long, help = "IP address (required with --force)")]
        ip: Option<String>,
        #[arg(long, help = "Bootstrap user (overrides inventory setting)")]
        user: Option<String>,
        #[arg(
            short = 'f',
            long,
            help = "Skip confirmation prompts (for CI/CD automation)"
        )]
        force: bool,
    },
}

fn validate_config_for_playbook(playbook_name: &str, tags: Option<&[String]>) -> Result<Preflight> {
    let config = Config::load()?;
    config.preflight_for(playbook_name, tags)
}

fn resolve_playbook_name(arg: &Path, playbooks: &[PathBuf]) -> Result<PathBuf> {
    let query = arg
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| eyre::eyre!("Invalid playbook name: {}", arg.display()))?;

    if let Some(found) = playbooks
        .iter()
        .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(query))
    {
        return Ok(found.clone());
    }

    let mut names: Vec<&str> = playbooks
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()))
        .collect();
    names.sort_unstable();

    eyre::bail!(
        "Playbook '{}' not found. Available playbooks: {}",
        query,
        names.join(", ")
    )
}

fn select_or_use_playbook(playbook_arg: Option<PathBuf>) -> Result<PathBuf> {
    match playbook_arg {
        Some(path) => {
            if path.is_file() {
                return Ok(path);
            }
            let playbooks = get_playbooks(None)?;
            resolve_playbook_name(&path, &playbooks)
        }
        None => {
            let playbooks = get_playbooks(None)?;
            select_item(
                &playbooks,
                |p: &PathBuf| {
                    let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                    let file = p.file_name().unwrap_or_default().to_string_lossy();
                    format!("{} ({})", name, file)
                },
                Choice::new("playbook").resolved_by("-p <playbook>"),
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_ansible_run(
    host: Option<String>,
    playbook: Option<PathBuf>,
    check: bool,
    tags: Option<Vec<String>>,
    skip_tags: Option<Vec<String>>,
    user: Option<String>,
    ask_pass: bool,
    force: bool,
) -> Result<()> {
    let selected_host = select_or_arg(host, HOST_FLAG)?;

    if let (None, Some(tag_list)) = (&playbook, &tags) {
        return run_auto_resolved(
            &selected_host,
            check,
            tag_list,
            skip_tags.as_deref(),
            user.as_deref(),
            ask_pass,
            force,
        );
    }

    let selected_playbook = select_or_use_playbook(playbook)?;
    run_single_playbook(
        &selected_host,
        &selected_playbook,
        check,
        tags.as_deref(),
        skip_tags.as_deref(),
        user.as_deref(),
        ask_pass,
        force,
    )
}

fn run_auto_resolved(
    host: &Host,
    check: bool,
    tags: &[String],
    skip_tags: Option<&[String]>,
    user: Option<&str>,
    ask_pass: bool,
    force: bool,
) -> Result<()> {
    let (runs, unresolved_tags) = resolve_tags_to_playbook_runs(tags)?;
    let (standalone_playbooks, unknown_tags) = split_standalone_redirects(unresolved_tags)?;

    if !unknown_tags.is_empty() {
        output::warn(&format!(
            "Unknown tags (no matching role, tag, or standalone playbook): {}",
            unknown_tags.join(", ")
        ));
    }

    if runs.is_empty() && standalone_playbooks.is_empty() {
        output::info("No auto-resolvable playbooks found, falling back to playbook selection");
        let selected_playbook = select_or_use_playbook(None)?;
        return run_single_playbook(
            host,
            &selected_playbook,
            check,
            Some(tags),
            skip_tags,
            user,
            ask_pass,
            force,
        );
    }

    output::info(&format!(
        "Resolved {} playbook run(s) for tags: {}",
        runs.len() + standalone_playbooks.len(),
        tags.join(", ")
    ));

    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    let app_versions = app_version_vars(&assets.playbooks_dir())?;
    let memory_budgets = app_memory_vars(&assets.playbooks_dir())?;
    let hosts_ignoreip = hosts_ignoreip_var()?;
    let mut extra_vars: Vec<(&str, &str)> = app_versions
        .iter()
        .chain(memory_budgets.iter())
        .chain(std::iter::once(&hosts_ignoreip))
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    if let Some(user) = user {
        extra_vars.push(("ansible_user", user));
    }

    for run in &runs {
        let playbook_file = run.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let playbook_stem = run
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let run_tags_ref = if run.tags.is_empty() {
            None
        } else {
            Some(run.tags.as_slice())
        };

        let preflight = validate_config_for_playbook(playbook_file, run_tags_ref)?;
        show_playbook_warnings(playbook_file, force)?;

        let run_tags = if run.tags.is_empty() {
            None
        } else {
            Some(run.tags.as_slice())
        };

        output::info(&format!(
            "Running {} on {}{}",
            playbook_stem,
            host.name,
            run_tags.map_or(String::new(), |t| format!(" (tags: {})", t.join(", ")))
        ));

        let inventory_host = InventoryHost {
            name: host.name.clone(),
            address: host.vars.ansible_host.clone(),
            port: host.vars.ansible_port,
            user: host.vars.bootstrap_user.clone(),
            groups: host.groups.clone(),
        };

        let mut progress = crate::services::progress::TerminalProgress::new("");
        let result = run_playbook(
            &preflight,
            &run.path,
            &inventory_host,
            check,
            run_tags,
            skip_tags,
            Some(&extra_vars),
            false,
            ask_pass,
            &mut progress,
        )?;

        if !result.success {
            if result.last_output.is_empty() {
                eyre::bail!(
                    "{} failed with exit code {}",
                    playbook_stem,
                    result.exit_code
                );
            } else {
                eyre::bail!(
                    "{} failed with exit code {}:\n{}",
                    playbook_stem,
                    result.exit_code,
                    result.last_output.trim()
                );
            }
        }

        output::success(&format!("{} completed successfully", playbook_stem));
    }

    for playbook in &standalone_playbooks {
        run_single_playbook(
            host, playbook, check, None, skip_tags, user, ask_pass, force,
        )?;
    }

    output::success("All playbook runs completed successfully");
    Ok(())
}

fn split_standalone_redirects(tags: Vec<String>) -> Result<(Vec<PathBuf>, Vec<String>)> {
    let mut playbooks = Vec::new();
    let mut unknown = Vec::new();
    for tag in tags {
        match find_standalone_playbook(&tag)? {
            Some(path) => playbooks.push(path),
            None => unknown.push(tag),
        }
    }
    Ok((playbooks, unknown))
}

fn ssh_password_notice(user: &str) -> String {
    format!(
        "The \"SSH password\" prompt below asks for the login password of user '{}' (not an SSH key passphrase)",
        user
    )
}

#[allow(clippy::too_many_arguments)]
fn run_single_playbook(
    host: &Host,
    playbook: &Path,
    check: bool,
    tags: Option<&[String]>,
    skip_tags: Option<&[String]>,
    user: Option<&str>,
    ask_pass: bool,
    force: bool,
) -> Result<()> {
    let playbook_file = playbook.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let playbook_stem = playbook
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let preflight = validate_config_for_playbook(playbook_file, tags)?;
    let is_fresh_bootstrap = playbook_file == "bootstrap.yml";

    if is_fresh_bootstrap {
        confirm_provider_firewall(&preflight, force)?;
    }

    show_playbook_warnings(playbook_file, force)?;

    output::info(&format!("Running {} on {}", playbook_stem, host.name));

    if is_fresh_bootstrap {
        output::info(&ssh_password_notice(
            user.unwrap_or(&host.vars.bootstrap_user),
        ));
    }

    let inventory_host = InventoryHost {
        name: host.name.clone(),
        address: host.vars.ansible_host.clone(),
        port: host.vars.ansible_port,
        user: host.vars.bootstrap_user.clone(),
        groups: host.groups.clone(),
    };

    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    let app_versions = app_version_vars(&assets.playbooks_dir())?;
    let memory_budgets = app_memory_vars(&assets.playbooks_dir())?;
    let hosts_ignoreip = hosts_ignoreip_var()?;
    let mut extra_vars: Vec<(&str, &str)> = app_versions
        .iter()
        .chain(memory_budgets.iter())
        .chain(std::iter::once(&hosts_ignoreip))
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    if let Some(user) = user {
        extra_vars.push(("ansible_user", user));
    }

    let mut progress = crate::services::progress::TerminalProgress::new("");
    let result = run_playbook(
        &preflight,
        playbook,
        &inventory_host,
        check,
        tags,
        skip_tags,
        Some(&extra_vars),
        false,
        ask_pass,
        &mut progress,
    )?;

    if result.success {
        output::success("Playbook completed successfully");
        Ok(())
    } else if result.last_output.is_empty() {
        eyre::bail!("Playbook failed with exit code {}", result.exit_code)
    } else {
        eyre::bail!(
            "Playbook failed with exit code {}:\n{}",
            result.exit_code,
            result.last_output.trim()
        )
    }
}

fn confirm_or_abort(question: &str, abort_message: &str, force: bool) -> Result<()> {
    if force {
        output::info("Skipping confirmation (--force enabled)");
        return Ok(());
    }

    eprint!("{} [y/N]: ", question);
    io::stderr().flush()?;
    let mut response = String::new();
    io::stdin().read_line(&mut response)?;

    if !response.trim().eq_ignore_ascii_case("y") {
        eprintln!("{}", abort_message);
        std::process::exit(1);
    }

    Ok(())
}

fn configured_ssh_port(flat_vars: &HashMap<String, String>) -> String {
    flat_vars
        .get("ssh_port")
        .cloned()
        .unwrap_or_else(|| "not configured".to_string())
}

fn confirm_provider_firewall(preflight: &Preflight, force: bool) -> Result<()> {
    eprintln!();
    output::info("IMPORTANT: Provider Firewall Configuration Required");
    output::info("Before running bootstrap, ensure your VPS provider's firewall");
    output::info("allows your custom SSH port (separate from UFW on the VPS)");
    eprintln!();
    output::info("Required steps:");
    output::info(&format!(
        "  1. Your target SSH port: {}",
        configured_ssh_port(preflight.flat_vars())
    ));
    output::info("  2. Log into your VPS provider dashboard (IONOS, etc.)");
    output::info("  3. Add firewall rule: Allow TCP on your SSH port");
    output::info("  4. Save and confirm the rule is active");
    eprintln!();
    output::info("Without this, you'll be locked out after SSH port change!");
    eprintln!();

    confirm_or_abort(
        "Have you configured your provider's firewall?",
        "Aborted. Configure provider firewall first, then re-run.",
        force,
    )
}

fn show_playbook_warnings(playbook_name: &str, force: bool) -> Result<()> {
    let needs_cloudflare_warning = playbook_name == "apps.yml";

    if needs_cloudflare_warning {
        eprintln!();
        output::info("IMPORTANT: Cloudflare API Token Configuration Required");
        output::info("Before running apps, ensure your Cloudflare API token has");
        output::info("the correct permissions for DNS-01 ACME challenges");
        eprintln!();
        output::info("Required steps:");
        output::info("  1. Log into Cloudflare: https://dash.cloudflare.com");
        output::info("  2. Navigate to: My Profile → API Tokens → Create Token");
        output::info("  3. Use 'Edit zone DNS' template");
        output::info("  4. Required permissions:");
        output::info("     - Zone → Zone → Read");
        output::info("     - Zone → DNS → Edit");
        output::info("  5. Set zone resources to your domain");
        output::info(
            "  6. Copy token and add: auberge config set cloudflare_dns_api_token <TOKEN>",
        );
        eprintln!();
        output::info("Note: IP whitelisting is optional (all IPs allowed by default)");
        eprintln!();
        output::info("Without this, SSL certificate generation will fail!");
        eprintln!();

        confirm_or_abort(
            "Have you configured your Cloudflare API token?",
            "Aborted. Configure Cloudflare API token first, then re-run.",
            force,
        )?;

        eprintln!();
        output::info("IMPORTANT: VPS Provider Firewall - Port 853 Required");
        output::info("For DNS over TLS with Blocky, your VPS provider's firewall");
        output::info("must allow incoming TCP connections on port 853");
        eprintln!();
        output::info("Required steps:");
        output::info("  1. Log into your VPS provider dashboard (IONOS, etc.)");
        output::info("  2. Navigate to firewall or security settings");
        output::info("  3. Add firewall rule: Allow TCP on port 853");
        output::info("  4. Save and confirm the rule is active");
        eprintln!();
        output::info("Without this, DNS over TLS will not be accessible!");
        eprintln!();

        confirm_or_abort(
            "Have you opened port 853 in your provider's firewall?",
            "Aborted. Open port 853 in provider firewall first, then re-run.",
            force,
        )?;
    }

    Ok(())
}

fn validate_ip(ip: &str) -> Result<()> {
    let ipv4_regex = Regex::new(r"^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$").unwrap();
    let ipv6_regex = Regex::new(r"^([0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}$").unwrap();

    if ipv4_regex.is_match(ip) {
        for octet_str in ipv4_regex.captures(ip).unwrap().iter().skip(1).flatten() {
            let octet: u16 = octet_str.as_str().parse().unwrap_or(256);
            if octet > 255 {
                eyre::bail!("Invalid IP format: {} (octet {} out of range)", ip, octet);
            }
        }
        Ok(())
    } else if ipv6_regex.is_match(ip) {
        Ok(())
    } else {
        eyre::bail!("Invalid IP format: {}", ip)
    }
}

fn offered_default(configured_address: &str) -> Option<&str> {
    Some(configured_address).filter(|addr| validate_ip(addr).is_ok())
}

fn resolve_prompted_ip(input: &str, default_ip: Option<&str>) -> Result<String> {
    let trimmed = input.trim();
    let candidate = match default_ip {
        Some(default) if trimmed.is_empty() => default,
        _ => trimmed,
    };
    validate_ip(candidate)?;
    Ok(candidate.to_string())
}

fn prompt_for_ip(host_name: &str, configured_address: &str) -> Result<String> {
    let default_ip = offered_default(configured_address);
    loop {
        match default_ip {
            Some(ip) => eprint!("Enter IP address for {} [{}]: ", host_name, ip),
            None => eprint!("Enter IP address for {}: ", host_name),
        }
        io::stderr().flush()?;
        let mut host_ip = String::new();
        let bytes_read = io::stdin()
            .read_line(&mut host_ip)
            .wrap_err("Failed to read IP address")?;
        match resolve_prompted_ip(&host_ip, default_ip) {
            Ok(ip) => return Ok(ip),
            Err(err) if bytes_read == 0 => return Err(err),
            Err(err) => output::warn(&err.to_string()),
        }
    }
}

pub fn run_ansible_bootstrap(
    host_arg: Option<String>,
    port: u16,
    ip: Option<String>,
    user: Option<String>,
    force: bool,
) -> Result<()> {
    let preflight = validate_config_for_playbook("bootstrap.yml", None)?;

    let host = select_or_arg(host_arg, HOST_POSITIONAL)?;
    let host_name = host.name.clone();
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    let bootstrap_playbook = assets.playbooks_dir().join("bootstrap.yml");

    if !bootstrap_playbook.exists() {
        eyre::bail!(
            "Bootstrap playbook not found: {}",
            bootstrap_playbook.display()
        );
    }

    confirm_provider_firewall(&preflight, force)?;

    let host_ip = match (ip, force) {
        (Some(ip_addr), _) => {
            validate_ip(&ip_addr)?;
            ip_addr
        }
        (None, true) => {
            eyre::bail!("--ip is required when using --force")
        }
        (None, false) => prompt_for_ip(&host_name, &host.vars.ansible_host)?,
    };

    let bootstrap_user = user
        .as_deref()
        .unwrap_or(&host.vars.bootstrap_user)
        .to_string();

    output::info(&format!(
        "Bootstrapping {} ({}) as {}",
        host_name, host_ip, bootstrap_user
    ));
    output::info(&ssh_password_notice(&bootstrap_user));

    let inventory_host = InventoryHost {
        name: host_name,
        address: host_ip,
        port,
        user: bootstrap_user,
        groups: host.groups.clone(),
    };

    let result = run_bootstrap(&preflight, &bootstrap_playbook, &inventory_host)?;

    if result.success {
        output::success("Bootstrap completed successfully");
        Ok(())
    } else {
        eyre::bail!("Bootstrap failed with exit code {}", result.exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configured_ssh_port_reads_flat_vars() {
        let mut vars = HashMap::new();
        vars.insert("ssh_port".to_string(), "2222".to_string());
        assert_eq!(configured_ssh_port(&vars), "2222");
    }

    #[test]
    fn test_configured_ssh_port_falls_back_when_unset() {
        assert_eq!(configured_ssh_port(&HashMap::new()), "not configured");
    }

    #[test]
    fn test_validate_ip_valid_ipv4() {
        assert!(validate_ip("192.168.1.1").is_ok());
        assert!(validate_ip("10.0.0.1").is_ok());
        assert!(validate_ip("172.16.0.1").is_ok());
        assert!(validate_ip("127.0.0.1").is_ok());
        assert!(validate_ip("0.0.0.0").is_ok());
        assert!(validate_ip("255.255.255.255").is_ok());
    }

    #[test]
    fn test_validate_ip_valid_ipv6() {
        assert!(validate_ip("::1").is_ok());
        assert!(validate_ip("2001:db8::1").is_ok());
        assert!(validate_ip("fe80::1").is_ok());
        assert!(validate_ip("::").is_ok());
        assert!(validate_ip("2001:0db8:85a3:0000:0000:8a2e:0370:7334").is_ok());
    }

    #[test]
    fn test_validate_ip_invalid_format() {
        assert!(validate_ip("999.999.999.999").is_err());
        assert!(validate_ip("192.168.1.256").is_err());
        assert!(validate_ip("not-an-ip").is_err());
        assert!(validate_ip("192.168.1").is_err());
        assert!(validate_ip("192.168.1.1.1").is_err());
        assert!(validate_ip("192.168.-1.1").is_err());
    }

    #[test]
    fn test_split_standalone_redirects_partitions_tags() {
        let (playbooks, unknown) =
            split_standalone_redirects(vec!["hermes".to_string(), "nope".to_string()]).unwrap();

        assert_eq!(playbooks.len(), 1);
        assert_eq!(
            playbooks[0].file_name().unwrap().to_str().unwrap(),
            "hermes.yml"
        );
        assert_eq!(unknown, vec!["nope"]);
    }

    #[test]
    fn test_split_standalone_redirects_keeps_aggregator_stems_unknown() {
        let (playbooks, unknown) = split_standalone_redirects(vec!["apps".to_string()]).unwrap();

        assert!(playbooks.is_empty());
        assert_eq!(unknown, vec!["apps"]);
    }

    #[test]
    fn test_validate_ip_edge_cases() {
        assert!(validate_ip("").is_err());
        assert!(validate_ip("   ").is_err());
        assert!(validate_ip("localhost").is_err());
        assert!(validate_ip("192.168.1.1 ").is_err());
        assert!(validate_ip(" 192.168.1.1").is_err());
    }

    #[test]
    fn test_resolve_prompted_ip_empty_input_accepts_default() {
        assert_eq!(
            resolve_prompted_ip("\n", Some("203.0.113.10")).unwrap(),
            "203.0.113.10"
        );
    }

    #[test]
    fn test_resolve_prompted_ip_whitespace_input_accepts_default() {
        assert_eq!(
            resolve_prompted_ip("   \n", Some("203.0.113.10")).unwrap(),
            "203.0.113.10"
        );
    }

    #[test]
    fn test_resolve_prompted_ip_trims_explicit_input() {
        assert_eq!(
            resolve_prompted_ip(" 10.0.0.5 \n", Some("203.0.113.10")).unwrap(),
            "10.0.0.5"
        );
    }

    #[test]
    fn test_resolve_prompted_ip_rejects_invalid_input() {
        let err = resolve_prompted_ip("999.999.999.999\n", Some("203.0.113.10")).unwrap_err();
        assert!(err.to_string().contains("Invalid IP format"));
    }

    #[test]
    fn test_resolve_prompted_ip_requires_explicit_input_without_default() {
        let err = resolve_prompted_ip("\n", None).unwrap_err();
        assert!(err.to_string().contains("Invalid IP format"));
    }

    #[test]
    fn test_offered_default_accepts_ip_addresses() {
        assert_eq!(offered_default("203.0.113.10"), Some("203.0.113.10"));
        assert_eq!(offered_default("2001:db8::1"), Some("2001:db8::1"));
    }

    #[test]
    fn test_offered_default_rejects_hostname_addresses() {
        assert_eq!(offered_default("vps.example.com"), None);
    }

    fn sample_playbooks() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/pb/hardening.yml"),
            PathBuf::from("/pb/infrastructure.yml"),
            PathBuf::from("/pb/apps.yml"),
            PathBuf::from("/pb/hermes.yml"),
        ]
    }

    #[test]
    fn test_resolve_playbook_name_bare() {
        let resolved = resolve_playbook_name(Path::new("hermes"), &sample_playbooks()).unwrap();
        assert_eq!(resolved, PathBuf::from("/pb/hermes.yml"));
    }

    #[test]
    fn test_resolve_playbook_name_with_yml_extension() {
        let resolved = resolve_playbook_name(Path::new("hermes.yml"), &sample_playbooks()).unwrap();
        assert_eq!(resolved, PathBuf::from("/pb/hermes.yml"));
    }

    #[test]
    fn test_resolve_playbook_name_ignores_leading_dirs() {
        let resolved =
            resolve_playbook_name(Path::new("some/dir/apps.yml"), &sample_playbooks()).unwrap();
        assert_eq!(resolved, PathBuf::from("/pb/apps.yml"));
    }

    #[test]
    fn test_resolve_playbook_name_unknown_lists_available() {
        let err = resolve_playbook_name(Path::new("nope"), &sample_playbooks()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Playbook 'nope' not found"));
        assert!(msg.contains("apps, hardening, hermes, infrastructure"));
    }

    #[test]
    fn test_ssh_password_notice_names_the_login_user() {
        let notice = ssh_password_notice("root");
        assert!(notice.contains("SSH password"));
        assert!(notice.contains("login password"));
        assert!(notice.contains("'root'"));
        assert!(notice.contains("not an SSH key passphrase"));
    }

    #[test]
    fn test_ssh_password_notice_interpolates_non_root_user() {
        assert!(ssh_password_notice("debian").contains("'debian'"));
    }
}
