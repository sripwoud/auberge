use crate::output;
use crate::services::dns::DnsService;
use clap::{Subcommand, ValueEnum};
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
        subdomain: String,
        #[arg(short, long, help = "IP address")]
        ip: String,
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
        service.config().domain,
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

pub async fn run_dns_set(subdomain: String, ip: String, production: bool) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;
    print_mode_banner();

    output::info(&format!(
        "Setting A record: {}.{} -> {}",
        subdomain,
        service.config().domain,
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
    use crate::services::dns::discover_subdomains;
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

    let mut discovered = discover_subdomains();

    if strict && discovered.is_empty() {
        eyre::bail!("No subdomain environment variables found");
    }

    let skip_set: HashSet<_> = skip.iter().cloned().collect();

    let subdomains_to_process: Vec<_> = if !subdomains.is_empty() {
        subdomains
            .into_iter()
            .filter(|s| !skip_set.contains(s))
            .filter_map(|s| discovered.remove(&s).map(|v| (s, v)))
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

    for (_, subdomain_value) in &subdomains_to_process {
        eprintln!(
            "  • {}.{} → {}",
            subdomain_value,
            service.config().domain,
            target_ip
        );
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

    for (idx, (_app_name, subdomain_value)) in subdomains_to_process.iter().enumerate() {
        match service.set_a_record(subdomain_value, &target_ip).await {
            Ok(_) => {
                output::success(&format!(
                    "Created {}.{}",
                    subdomain_value,
                    service.config().domain
                ));
                succeeded += 1;
            }
            Err(e) => {
                eprintln!(
                    "Failed {}.{}: {}",
                    subdomain_value,
                    service.config().domain,
                    e
                );
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

    output::success(&format!(
        "Successfully created {}/{} A records pointing to {}",
        succeeded,
        subdomains_to_process.len(),
        target_ip
    ));

    if failed > 0 {
        eprintln!("Failed to create {} records", failed);
        std::process::exit(1);
    }

    Ok(())
}
