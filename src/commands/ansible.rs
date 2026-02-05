use crate::models::inventory::Host;
use crate::models::playbook::Playbook;
use crate::output;
use crate::selector::select_item;
use crate::services::ansible_runner::{run_bootstrap, run_playbook};
use crate::services::inventory::{get_host, get_hosts, get_playbooks};
use clap::Subcommand;
use eyre::{Result, WrapErr};
use regex::Regex;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AnsibleCommands {
    #[command(alias = "r")]
    Run {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Playbook path")]
        playbook: Option<PathBuf>,
        #[arg(short = 'C', long, help = "Run in check mode (dry run)")]
        check: bool,
        #[arg(short, long, help = "Only run tasks with these tags")]
        tags: Option<Vec<String>>,
        #[arg(
            short = 'f',
            long,
            help = "Skip confirmation prompts (for CI/CD automation)"
        )]
        force: bool,
    },
    #[command(alias = "c")]
    Check {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Playbook path")]
        playbook: Option<PathBuf>,
        #[arg(
            short = 'f',
            long,
            help = "Skip confirmation prompts (for CI/CD automation)"
        )]
        force: bool,
    },
    #[command(alias = "b")]
    Bootstrap {
        host: String,
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

fn select_or_use_host(host_arg: Option<String>) -> Result<Host> {
    match host_arg {
        Some(name) => get_host(&name, None),
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

fn select_or_use_playbook(playbook_arg: Option<PathBuf>) -> Result<Playbook> {
    match playbook_arg {
        Some(path) => Ok(Playbook::from_path(path)),
        None => {
            let playbooks = get_playbooks(None)?;
            select_item(
                &playbooks,
                |p: &Playbook| {
                    format!(
                        "{} ({})",
                        p.name,
                        p.path.file_name().unwrap_or_default().to_string_lossy()
                    )
                },
                "Select playbook",
            )?
            .ok_or_else(|| eyre::eyre!("No playbook selected"))
        }
    }
}

pub fn run_ansible_run(
    host: Option<String>,
    playbook: Option<PathBuf>,
    check: bool,
    tags: Option<Vec<String>>,
    force: bool,
) -> Result<()> {
    let selected_host = select_or_use_host(host)?;
    let selected_playbook = select_or_use_playbook(playbook)?;

    // Show warning for fresh bootstrap only (not for full auberge.yml deployments)
    let playbook_name = selected_playbook
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let is_fresh_bootstrap = playbook_name == "bootstrap.yml";

    if is_fresh_bootstrap {
        eprintln!();
        output::info("IMPORTANT: Provider Firewall Configuration Required");
        output::info("Before running bootstrap, ensure your VPS provider's firewall");
        output::info("allows your custom SSH port (separate from UFW on the VPS)");
        eprintln!();
        output::info("Required steps:");
        output::info("  1. Get your SSH_PORT: mise env | grep SSH_PORT");
        output::info("  2. Log into your VPS provider dashboard (IONOS, etc.)");
        output::info("  3. Add firewall rule: Allow TCP on your SSH_PORT");
        output::info("  4. Save and confirm the rule is active");
        eprintln!();
        output::info("Without this, you'll be locked out after SSH port change!");
        eprintln!();

        if !force {
            print!("Have you configured your provider's firewall? [y/N]: ");
            io::stdout().flush()?;
            let mut response = String::new();
            io::stdin().read_line(&mut response)?;

            if !response.trim().eq_ignore_ascii_case("y") {
                eprintln!("Aborted. Configure provider firewall first, then re-run.");
                std::process::exit(1);
            }
        } else {
            output::info("Skipping confirmation (--force enabled)");
        }
    }

    // Show Cloudflare API configuration warning for apps playbook
    let needs_cloudflare_warning = playbook_name == "apps.yml" || playbook_name == "auberge.yml";

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
            "  6. Copy token and add: mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN",
        );
        eprintln!();
        output::info("Note: IP whitelisting is optional (all IPs allowed by default)");
        eprintln!();
        output::info("Without this, SSL certificate generation will fail!");
        eprintln!();

        if !force {
            print!("Have you configured your Cloudflare API token? [y/N]: ");
            io::stdout().flush()?;
            let mut response = String::new();
            io::stdin().read_line(&mut response)?;

            if !response.trim().eq_ignore_ascii_case("y") {
                eprintln!("Aborted. Configure Cloudflare API token first, then re-run.");
                std::process::exit(1);
            }
        } else {
            output::info("Skipping confirmation (--force enabled)");
        }

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

        if !force {
            print!("Have you opened port 853 in your provider's firewall? [y/N]: ");
            io::stdout().flush()?;
            let mut port_response = String::new();
            io::stdin().read_line(&mut port_response)?;

            if !port_response.trim().eq_ignore_ascii_case("y") {
                eprintln!("Aborted. Open port 853 in provider firewall first, then re-run.");
                std::process::exit(1);
            }
        } else {
            output::info("Skipping confirmation (--force enabled)");
        }
    }

    output::info(&format!(
        "Running {} on {}",
        selected_playbook.name, selected_host.name
    ));

    let result = run_playbook(
        &selected_playbook.path,
        &selected_host.name,
        check,
        tags.as_deref(),
        None,
        false,
    )?;

    if result.success {
        output::success("Playbook completed successfully");
        Ok(())
    } else {
        eyre::bail!("Playbook failed with exit code {}", result.exit_code)
    }
}

pub fn run_ansible_check(
    host: Option<String>,
    playbook: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    run_ansible_run(host, playbook, true, None, force)
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

fn prompt_for_ip(host_name: &str) -> Result<String> {
    print!("Enter IP address for {}: ", host_name);
    io::stdout().flush()?;
    let mut host_ip = String::new();
    io::stdin()
        .read_line(&mut host_ip)
        .wrap_err("Failed to read IP address")?;
    Ok(host_ip.trim().to_string())
}

pub fn run_ansible_bootstrap(
    host_name: String,
    port: u16,
    ip: Option<String>,
    user: Option<String>,
    force: bool,
) -> Result<()> {
    let host = get_host(&host_name, None)?;
    let bootstrap_playbook =
        crate::services::inventory::find_project_root().join("ansible/playbooks/bootstrap.yml");

    if !bootstrap_playbook.exists() {
        eyre::bail!(
            "Bootstrap playbook not found: {}",
            bootstrap_playbook.display()
        );
    }

    let host_ip = match (ip, force) {
        (Some(ip_addr), _) => {
            validate_ip(&ip_addr)?;
            ip_addr
        }
        (None, true) => {
            eyre::bail!("--ip is required when using --force")
        }
        (None, false) => prompt_for_ip(&host_name)?,
    };

    let bootstrap_user = user.as_deref().unwrap_or(&host.vars.bootstrap_user);

    output::info(&format!(
        "Bootstrapping {} ({}) as {}",
        host_name, host_ip, bootstrap_user
    ));

    let result = run_bootstrap(
        &bootstrap_playbook,
        &host_name,
        &host_ip,
        bootstrap_user,
        port,
    )?;

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
    fn test_validate_ip_edge_cases() {
        assert!(validate_ip("").is_err());
        assert!(validate_ip("   ").is_err());
        assert!(validate_ip("localhost").is_err());
        assert!(validate_ip("192.168.1.1 ").is_err());
        assert!(validate_ip(" 192.168.1.1").is_err());
    }
}
