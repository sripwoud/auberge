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
    #[command(about = "List DNS records")]
    List {
        #[arg(short, long, help = "Filter by subdomain name")]
        subdomain: Option<String>,
    },
    #[command(about = "Show DNS status and health")]
    Status,
    #[command(about = "Set an A record for a subdomain")]
    Set {
        #[arg(short, long, help = "Subdomain name")]
        subdomain: String,
        #[arg(short, long, help = "IP address")]
        ip: String,
    },
    #[command(about = "Migrate all A records to a new IP")]
    Migrate {
        #[arg(short, long, help = "New IP address")]
        ip: String,
        #[arg(short = 'n', long, help = "Dry run (don't actually migrate)")]
        dry_run: bool,
    },
    #[command(
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
    },
}

pub async fn run_dns_list(subdomain: Option<String>) -> Result<()> {
    let service = DnsService::new()?;
    let records = service.list_records().await?;

    let filtered: Vec<_> = match &subdomain {
        Some(name) => records.iter().filter(|r| r.name == *name).collect(),
        None => records.iter().collect(),
    };

    if filtered.is_empty() {
        eprintln!("No DNS records found");
        return Ok(());
    }

    eprintln!(
        "DNS Records for {}\n{:<14} {:<8} {:<24} {:>6}",
        service.config().domain,
        "NAME",
        "TYPE",
        "ADDRESS",
        "TTL"
    );
    eprintln!("{}", "-".repeat(56));

    for record in filtered {
        eprintln!(
            "{:<14} {:<8} {:<24} {:>6}",
            record.name, record.type_, record.address, record.ttl
        );
    }

    Ok(())
}

pub async fn run_dns_status() -> Result<()> {
    let service = DnsService::new()?;
    let status = service.status().await?;

    eprintln!("DNS Status for {}", status.domain);
    eprintln!("{}", "-".repeat(40));

    eprintln!(
        "\nConfigured subdomains: {}",
        status.configured_subdomains.join(", ")
    );

    let a_records: Vec<_> = status
        .active_records
        .iter()
        .filter(|r| r.type_ == "A")
        .collect();

    eprintln!("\nActive A records: {}", a_records.len());
    for record in &a_records {
        eprintln!("  {} -> {}", record.name, record.address);
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

pub async fn run_dns_set(subdomain: String, ip: String) -> Result<()> {
    let service = DnsService::new()?;

    eprintln!(
        "Setting A record: {}.{} -> {}",
        subdomain,
        service.config().domain,
        ip
    );

    service.set_a_record(&subdomain, &ip).await?;
    eprintln!("Done");

    Ok(())
}

pub async fn run_dns_migrate(ip: String, dry_run: bool) -> Result<()> {
    let service = DnsService::new()?;

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
) -> Result<()> {
    use crate::services::dns::discover_subdomains;
    use crate::services::inventory::discover_hosts_with_ips;
    use std::collections::HashSet;

    let service = DnsService::new()?;

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
        eprintln!("No subdomains to process");
        return Ok(());
    }

    if dry_run {
        eprintln!("[DRY RUN] Would create the following A records:");
    } else {
        eprintln!("Creating the following A records:");
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
            eprintln!("Operation cancelled");
            return Ok(());
        }
    }

    if dry_run {
        eprintln!("\n[DRY RUN] No changes were made");
        return Ok(());
    }

    eprintln!();
    let mut succeeded = 0;
    let mut failed = 0;

    for (_app_name, subdomain_value) in &subdomains_to_process {
        match service.set_a_record(subdomain_value, &target_ip).await {
            Ok(_) => {
                eprintln!(
                    "  ✓ Created {}.{}",
                    subdomain_value,
                    service.config().domain
                );
                succeeded += 1;
            }
            Err(e) => {
                eprintln!(
                    "  ✗ Failed {}.{}: {}",
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
    }

    eprintln!(
        "\n✓ Successfully created {}/{} A records pointing to {}",
        succeeded,
        subdomains_to_process.len(),
        target_ip
    );

    if failed > 0 {
        eprintln!("✗ Failed to create {} records", failed);
        std::process::exit(1);
    }

    Ok(())
}
