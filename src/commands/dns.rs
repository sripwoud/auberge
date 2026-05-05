use crate::output;
use crate::prompt::select_item;
use crate::services::dns::DnsService;
use clap::{Subcommand, ValueEnum};
use dialoguer::{Input, theme::ColorfulTheme};
use eyre::Result;

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
    Tsv,
}

#[derive(Subcommand)]
pub enum DnsCommands {
    #[command(alias = "l", about = "List DNS records")]
    List {
        #[arg(short, long, help = "Filter by subdomain name")]
        subdomain: Option<String>,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
    #[command(alias = "st", about = "Show DNS status and health")]
    Status {
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
    #[command(alias = "s", about = "Set an A record for a subdomain")]
    Set {
        #[arg(short, long, help = "Subdomain name")]
        subdomain: Option<String>,
        #[arg(short, long, help = "IP address")]
        ip: Option<String>,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
    #[command(alias = "m", about = "Migrate all A records to a new IP")]
    Migrate {
        #[arg(short, long, help = "New IP address")]
        ip: String,
        #[arg(short = 'n', long, help = "Dry run (don't actually migrate)")]
        dry_run: bool,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
    #[command(
        alias = "sa",
        about = "Batch create A records for all app subdomains",
        long_about = "Interactively or automatically create DNS A records for all configured \
                      app subdomains (blocky, calibre, freshrss, etc.) pointing to a selected \
                      host's IP address."
    )]
    SetAll {
        #[arg(
            short = 'H',
            long,
            value_name = "HOST",
            help = "Target host (auberge, auberge-old, vibecoder)"
        )]
        host: Option<String>,
        #[arg(
            short,
            long,
            value_name = "IP",
            conflicts_with = "host",
            help = "Override IP address"
        )]
        ip: Option<String>,
        #[arg(short = 'n', long, help = "Preview changes without executing")]
        dry_run: bool,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
        #[arg(
            short,
            long,
            help = "Fail if any subdomain env var is missing (non-interactive)"
        )]
        strict: bool,
        #[arg(
            short = 'S',
            long,
            value_name = "NAMES",
            value_delimiter = ',',
            help = "Only process specific subdomains (comma-separated)"
        )]
        subdomains: Vec<String>,
        #[arg(
            long,
            value_name = "NAMES",
            value_delimiter = ',',
            help = "Skip specific subdomains (comma-separated)"
        )]
        skip: Vec<String>,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
        #[arg(long, help = "Continue on errors instead of failing fast")]
        continue_on_error: bool,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
}

pub async fn run_dns_list(subdomain: Option<String>, production: bool) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;
    print_mode_banner();

    let records = service.list_records().await?;

    let filtered: Vec<_> = match &subdomain {
        Some(name) => records.iter().filter(|r| r.name == *name).collect(),
        None => records.iter().collect(),
    };

    if filtered.is_empty() {
        output::info("No DNS records found");
        return Ok(());
    }

    eprintln!(
        "DNS Records for {}\n{:<40} {:<8} {:<24} {:>6}",
        service.domain(),
        "NAME",
        "TYPE",
        "CONTENT",
        "TTL"
    );
    eprintln!("{}", "-".repeat(80));

    for record in filtered {
        let (record_type, content) = format_dns_content(&record.content);
        eprintln!(
            "{:<40} {:<8} {:<24} {:>6}",
            record.name, record_type, content, record.ttl
        );
    }

    Ok(())
}

fn format_dns_content(content: &cloudflare::endpoints::dns::dns::DnsContent) -> (String, String) {
    use cloudflare::endpoints::dns::dns::DnsContent;
    match content {
        DnsContent::A { content } => ("A".to_string(), content.to_string()),
        DnsContent::AAAA { content } => ("AAAA".to_string(), content.to_string()),
        DnsContent::CNAME { content } => ("CNAME".to_string(), content.clone()),
        DnsContent::MX { content, priority } => {
            ("MX".to_string(), format!("{} ({})", content, priority))
        }
        DnsContent::TXT { content } => ("TXT".to_string(), content.clone()),
        DnsContent::NS { content } => ("NS".to_string(), content.clone()),
        DnsContent::SRV { content } => ("SRV".to_string(), content.clone()),
    }
}

fn print_mode_banner() {
    output::info("CLOUDFLARE DNS");
}

pub async fn run_dns_status(production: bool) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;
    print_mode_banner();

    let status = service.status().await?;

    eprintln!("DNS Status for {}", status.domain);
    eprintln!("{}", "-".repeat(40));

    eprintln!(
        "\nConfigured subdomains: {}",
        status.configured_subdomains.join(", ")
    );

    use cloudflare::endpoints::dns::dns::DnsContent;
    let a_records: Vec<_> = status
        .active_records
        .iter()
        .filter(|r| matches!(r.content, DnsContent::A { .. }))
        .collect();

    eprintln!("\nActive A records: {}", a_records.len());
    for record in &a_records {
        if let DnsContent::A { content } = record.content {
            eprintln!("  {} -> {}", record.name, content);
        }
    }

    if !status.missing_subdomains.is_empty() {
        eprintln!(
            "\nMissing subdomains: {}",
            status.missing_subdomains.join(", ")
        );
    } else {
        eprintln!("\nAll configured subdomains have A records");
    }

    Ok(())
}

fn resolve_subdomain(subdomain: Option<String>) -> Result<String> {
    match subdomain {
        Some(s) => Ok(s),
        None => {
            crate::config::Config::load()?;
            let subdomains = crate::services::dns::discover_subdomains();
            let mut items: Vec<String> = subdomains.values().map(|e| e.subdomain.clone()).collect();
            if items.is_empty() {
                eyre::bail!("No subdomains defined in config");
            }
            items.sort();
            items.dedup();
            select_item(&items, |s: &String| s.clone(), "Select subdomain")?
                .ok_or_else(|| eyre::eyre!("No subdomain selected"))
        }
    }
}

fn resolve_ip(ip: Option<String>) -> Result<String> {
    match ip {
        Some(i) => Ok(i),
        None => {
            let value = Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("IP address")
                .interact_text()?;
            let value = value.trim().to_string();
            value
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| eyre::eyre!("Invalid IPv4 address: {}", value))?;
            Ok(value)
        }
    }
}

pub async fn run_dns_set(
    subdomain: Option<String>,
    ip: Option<String>,
    production: bool,
) -> Result<()> {
    let subdomain = resolve_subdomain(subdomain)?;
    let ip = resolve_ip(ip)?;

    let service = DnsService::new_with_production(Some(production)).await?;
    print_mode_banner();

    output::info(&format!(
        "Setting A record: {}.{} -> {}",
        subdomain,
        service.domain(),
        ip
    ));

    service.set_a_record(&subdomain, &ip).await?;
    output::success("A record set successfully");

    Ok(())
}

pub async fn run_dns_migrate(ip: String, dry_run: bool, production: bool) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;
    print_mode_banner();

    if dry_run {
        eprintln!("[DRY RUN] DNS Migration Preview");
    } else {
        eprintln!("DNS Migration");
    }
    eprintln!("{}", "-".repeat(50));
    eprintln!(
        "{:<14} {:<16} {:^3} {:<16}",
        "SUBDOMAIN", "CURRENT", "", "NEW"
    );
    eprintln!("{}", "-".repeat(50));

    let results = service.migrate_all(&ip, dry_run).await?;

    for result in &results {
        eprintln!(
            "{:<14} {:<16} ->  {:<16}",
            result.subdomain, result.old_ip, result.new_ip
        );
    }

    if dry_run {
        eprintln!("\nWould update {} A record(s).", results.len());
    } else {
        let success_count = results.iter().filter(|r| r.success).count();
        eprintln!("\nUpdated {} A record(s).", success_count);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_dns_set_all(
    host: Option<String>,
    ip: Option<String>,
    dry_run: bool,
    yes: bool,
    strict: bool,
    subdomains: Vec<String>,
    skip: Vec<String>,
    _output: OutputFormat,
    continue_on_error: bool,
    production: bool,
) -> Result<()> {
    use crate::hosts::HostManager;
    use crate::services::dns::{SubdomainEntry, discover_subdomains};
    use crate::services::inventory::discover_hosts_with_ips;
    use std::collections::HashSet;

    let service = DnsService::new_with_production(Some(production)).await?;
    print_mode_banner();

    let target_ip = match (&host, &ip) {
        (Some(host_name), None) => {
            let hosts = discover_hosts_with_ips(None)?;
            hosts
                .get(host_name)
                .ok_or_else(|| {
                    eyre::eyre!(
                        "Host '{}' not found in inventory. Available: {}",
                        host_name,
                        hosts.keys().cloned().collect::<Vec<_>>().join(", ")
                    )
                })?
                .clone()
        }
        (None, Some(ip_addr)) => ip_addr.clone(),
        (None, None) => {
            if !strict {
                eyre::bail!("Either --host or --ip must be specified");
            } else {
                eyre::bail!("Either --host or --ip must be specified in strict mode");
            }
        }
        _ => unreachable!(),
    };

    // When `--host` is used, look up the host's cached Tailscale IP so we can
    // auto-fill `ip_override` for tailnet-only apps. With `--ip` the user is
    // being explicit; we don't second-guess.
    //
    // `load_hosts()?` propagates parse/IO errors so a malformed `hosts.toml`
    // surfaces here instead of being silently treated as "no cached IP" —
    // missing-host vs malformed-file diverge intentionally.
    let host_tailscale_ip: Option<String> = if let Some(name) = host.as_deref() {
        HostManager::load_hosts()?
            .into_iter()
            .find(|h| h.name == name)
            .and_then(|h| h.tailscale_ip)
    } else {
        None
    };

    let mut discovered = discover_subdomains();

    apply_tailnet_only_fallback(&mut discovered, host_tailscale_ip.as_deref())?;

    if strict && discovered.is_empty() {
        eyre::bail!("No subdomain environment variables found");
    }

    let skip_set: HashSet<_> = skip.iter().cloned().collect();

    let subdomains_to_process: Vec<(String, SubdomainEntry)> = if !subdomains.is_empty() {
        subdomains
            .into_iter()
            .filter(|s| !skip_set.contains(s))
            .filter_map(|s| discovered.remove(&s).map(|entry| (s, entry)))
            .collect()
    } else {
        discovered
            .into_iter()
            .filter(|(k, _)| !skip_set.contains(k))
            .collect()
    };

    if subdomains_to_process.is_empty() {
        output::info("No subdomains to process");
        return Ok(());
    }

    if dry_run {
        output::info("DRY RUN - Would create the following A records:");
    } else {
        output::info("Creating the following A records:");
    }

    for (_, entry) in &subdomains_to_process {
        let effective_ip = entry.ip_override.as_deref().unwrap_or(&target_ip);
        if entry.ip_override.is_some() {
            eprintln!(
                "  • {}.{} → {} (tailnet)",
                entry.subdomain,
                service.domain(),
                effective_ip
            );
        } else {
            eprintln!(
                "  • {}.{} → {}",
                entry.subdomain,
                service.domain(),
                effective_ip
            );
        }
    }

    if !yes && !dry_run {
        eprint!("\nProceed? [y/N]: ");
        use std::io::{self, BufRead};
        let mut response = String::new();
        io::stdin().lock().read_line(&mut response)?;
        if !response.trim().eq_ignore_ascii_case("y") {
            println!("Operation cancelled");
            return Ok(());
        }
    }

    if dry_run {
        output::info("DRY RUN - No changes were made");
        return Ok(());
    }

    eprintln!();
    let mut succeeded = 0;
    let mut failed = 0;

    for (idx, (_app_name, entry)) in subdomains_to_process.iter().enumerate() {
        let effective_ip = entry.ip_override.as_deref().unwrap_or(&target_ip);
        match service.set_a_record(&entry.subdomain, effective_ip).await {
            Ok(_) => {
                output::success(&format!("Created {}.{}", entry.subdomain, service.domain()));
                succeeded += 1;
            }
            Err(e) => {
                eprintln!("Failed {}.{}: {}", entry.subdomain, service.domain(), e);
                failed += 1;
                if !continue_on_error {
                    return Err(e);
                }
            }
        }

        if idx < subdomains_to_process.len() - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    let has_overrides = subdomains_to_process
        .iter()
        .any(|(_, e)| e.ip_override.is_some());
    if has_overrides {
        output::success(&format!(
            "Successfully created {}/{} A records (some with tailnet IP overrides)",
            succeeded,
            subdomains_to_process.len(),
        ));
    } else {
        output::success(&format!(
            "Successfully created {}/{} A records pointing to {}",
            succeeded,
            subdomains_to_process.len(),
            target_ip
        ));
    }

    if failed > 0 {
        eprintln!("Failed to create {} records", failed);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::apply_tailnet_only_fallback;
    use crate::services::dns::SubdomainEntry;
    use std::collections::HashMap;

    fn entry(subdomain: &str, ip_override: Option<&str>) -> SubdomainEntry {
        SubdomainEntry {
            subdomain: subdomain.to_string(),
            ip_override: ip_override.map(String::from),
        }
    }

    #[test]
    fn fills_tailnet_only_app_when_host_has_tailscale_ip() {
        let mut discovered = HashMap::new();
        discovered.insert("bichon".to_string(), entry("bichon", None));

        apply_tailnet_only_fallback(&mut discovered, Some("100.64.0.5")).unwrap();

        assert_eq!(
            discovered["bichon"].ip_override.as_deref(),
            Some("100.64.0.5")
        );
    }

    #[test]
    fn preserves_explicit_override_for_tailnet_only_app() {
        let mut discovered = HashMap::new();
        discovered.insert("bichon".to_string(), entry("bichon", Some("100.42.42.42")));

        apply_tailnet_only_fallback(&mut discovered, Some("100.64.0.5")).unwrap();

        assert_eq!(
            discovered["bichon"].ip_override.as_deref(),
            Some("100.42.42.42"),
        );
    }

    #[test]
    fn leaves_public_app_unchanged_even_with_host_tailscale_ip() {
        let mut discovered = HashMap::new();
        discovered.insert("freshrss".to_string(), entry("rss", None));

        apply_tailnet_only_fallback(&mut discovered, Some("100.64.0.5")).unwrap();

        assert!(discovered["freshrss"].ip_override.is_none());
    }

    #[test]
    fn fails_fast_when_tailnet_only_app_has_no_tailscale_ip() {
        let mut discovered = HashMap::new();
        discovered.insert("bichon".to_string(), entry("bichon", None));

        let err = apply_tailnet_only_fallback(&mut discovered, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bichon"));
        assert!(msg.contains("tailnet-only"));
    }

    #[test]
    fn ignores_apps_with_missing_meta_files() {
        let mut discovered = HashMap::new();
        discovered.insert(
            "nonexistent_app_xyz".to_string(),
            entry("nonexistent_app_xyz", None),
        );

        apply_tailnet_only_fallback(&mut discovered, Some("100.64.0.5")).unwrap();

        assert!(discovered["nonexistent_app_xyz"].ip_override.is_none());
    }
}

/// For each discovered subdomain whose playbook meta declares
/// `tailnet_only: true` and which lacks an explicit `<app>_tailscale_ip`
/// override, fill `ip_override` from the host's cached Tailscale IP.
///
/// Bails when a tailnet-only app has no resolvable Tailscale IP — pointing
/// such an app at the host's public IP would be silently broken (Caddy binds
/// only to the Tailscale interface), so failing fast surfaces the missing
/// configuration to the user.
fn apply_tailnet_only_fallback(
    discovered: &mut std::collections::HashMap<String, crate::services::dns::SubdomainEntry>,
    host_tailscale_ip: Option<&str>,
) -> Result<()> {
    use crate::playbook_meta::PlaybookMeta;

    for (app, entry) in discovered.iter_mut() {
        if entry.ip_override.is_some() {
            continue;
        }
        let Some(meta) = PlaybookMeta::load_for_app(app)? else {
            continue;
        };
        if !meta.tailnet_only {
            continue;
        }
        match host_tailscale_ip {
            Some(ip) => entry.ip_override = Some(ip.to_string()),
            None => eyre::bail!(
                "App '{app}' is tailnet-only but no Tailscale IP is available. \
                 Either pass --host (after running `auberge host detect-tailscale-ip <name>`), \
                 or set `{app}_tailscale_ip` in config.toml."
            ),
        }
    }
    Ok(())
}
