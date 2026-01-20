use crate::services::dns::DnsService;
use clap::Subcommand;
use eyre::Result;

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
