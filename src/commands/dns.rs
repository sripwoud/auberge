use crate::output;
use crate::output::OutputFormat;
use crate::prompt::select_item;
use crate::services::dns::DnsService;
use clap::Subcommand;
use dialoguer::{Input, theme::ColorfulTheme};
use eyre::Result;
use serde::Serialize;

#[derive(Subcommand)]
pub enum DnsCommands {
    #[command(alias = "l", about = "List DNS records")]
    List {
        #[arg(short, long, help = "Filter by subdomain name")]
        subdomain: Option<String>,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
    #[command(alias = "st", about = "Show DNS status and health")]
    Status {
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
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
    #[command(
        alias = "d",
        about = "Delete an A record for a subdomain",
        long_about = "Delete the Cloudflare A record for a subdomain.\n\n\
                      Idempotent — running against an already-absent record reports success. \
                      Only A records are considered; CNAME / AAAA / TXT records for the same \
                      name are left untouched.\n\n\
                      Confirmation is required by default; --yes skips it. Production deletions \
                      escalate the confirmation: the user must retype the subdomain name to \
                      proceed.\n\n\
                      EXAMPLES:\n  \
                      # Pick a subdomain interactively, confirm, then delete (sandbox)\n  \
                      auberge dns delete\n\n  \
                      # Preview the action without deleting\n  \
                      auberge dns delete -s freshrss --dry-run\n\n  \
                      # Production delete in CI (no prompts)\n  \
                      auberge dns delete -s freshrss --production --yes"
    )]
    Delete {
        #[arg(short, long, help = "Subdomain name (omit to be prompted)")]
        subdomain: Option<String>,
        #[arg(short = 'n', long, help = "Preview without deleting")]
        dry_run: bool,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
    },
    #[command(alias = "m", about = "Migrate all A records to a new IP")]
    Migrate {
        #[arg(short, long, help = "New IP address")]
        ip: String,
        #[arg(short = 'n', long, help = "Dry run (don't actually migrate)")]
        dry_run: bool,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
        #[arg(short = 'P', long, help = "Use production API (default: sandbox)")]
        production: bool,
    },
    #[command(
        alias = "sa",
        about = "Batch create A records for all app subdomains",
        long_about = "Interactively or automatically create DNS A records for all configured \
                      app subdomains pointing to a selected host's IP address.\n\n\
                      Tailnet-only apps (playbook meta `tailnet_only: true`) are handled \
                      automatically per ADR-0003:\n\n\
                      • Implicit discovery (no --subdomains): tailnet-only apps are skipped \
                        automatically; a grouped info line is emitted to stderr.\n\
                      • Explicit target (--subdomains names a tailnet-only app): hard-error \
                        before any Cloudflare API call; use `auberge deploy <app>` instead.\n\n\
                      EXAMPLES:\n  \
                      # Publish all Public Apps; tailnet-only apps are skipped automatically\n  \
                      auberge dns set-all --host auberge --production\n\n  \
                      # Dry-run preview\n  \
                      auberge dns set-all --host auberge --dry-run\n\n  \
                      # Only specific apps (all must be public)\n  \
                      auberge dns set-all --host auberge --subdomains freshrss,baikal"
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

#[derive(Serialize)]
struct DnsRecordRow {
    name: String,
    record_type: String,
    content: String,
    ttl: u32,
}

pub async fn run_dns_list(
    subdomain: Option<String>,
    output: OutputFormat,
    production: bool,
) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;

    let records = service.list_records().await?;

    let filtered: Vec<_> = match &subdomain {
        Some(name) => records.iter().filter(|r| r.name == *name).collect(),
        None => records.iter().collect(),
    };

    match output {
        OutputFormat::Json => {
            let rows: Vec<DnsRecordRow> = filtered
                .iter()
                .map(|r| {
                    let (record_type, content) = format_dns_content(&r.content);
                    DnsRecordRow {
                        name: r.name.clone(),
                        record_type,
                        content,
                        ttl: r.ttl,
                    }
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        OutputFormat::Human => {
            print_mode_banner();
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
        }
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

#[derive(Serialize)]
struct StatusARecord {
    name: String,
    ip: String,
}

#[derive(Serialize)]
struct DnsStatusJson {
    domain: String,
    configured_subdomains: Vec<String>,
    active_a_records: Vec<StatusARecord>,
    missing_subdomains: Vec<String>,
}

pub async fn run_dns_status(output: OutputFormat, production: bool) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;
    let status = service.status().await?;

    use cloudflare::endpoints::dns::dns::DnsContent;
    let a_records: Vec<_> = status
        .active_records
        .iter()
        .filter(|r| matches!(r.content, DnsContent::A { .. }))
        .collect();

    match output {
        OutputFormat::Json => {
            let json_status = DnsStatusJson {
                domain: status.domain.clone(),
                configured_subdomains: status.configured_subdomains.clone(),
                active_a_records: a_records
                    .iter()
                    .filter_map(|r| {
                        if let DnsContent::A { content } = r.content {
                            Some(StatusARecord {
                                name: r.name.clone(),
                                ip: content.to_string(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect(),
                missing_subdomains: status.missing_subdomains.clone(),
            };
            println!("{}", serde_json::to_string_pretty(&json_status)?);
        }
        OutputFormat::Human => {
            print_mode_banner();
            eprintln!("DNS Status for {}", status.domain);
            eprintln!("{}", "-".repeat(40));
            eprintln!(
                "\nConfigured subdomains: {}",
                status.configured_subdomains.join(", ")
            );
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
        }
    }

    Ok(())
}

fn resolve_subdomain(subdomain: Option<String>) -> Result<String> {
    use std::io::IsTerminal;
    match subdomain {
        Some(s) => Ok(s),
        None => {
            if !std::io::stdin().is_terminal() {
                eyre::bail!("No subdomain provided. Pass -s <name> for non-interactive use.");
            }
            crate::config::Config::load()?;
            let subdomains = crate::services::dns::discover_subdomains();
            let mut items: Vec<String> = subdomains.values().map(|e| e.subdomain.clone()).collect();
            if items.is_empty() {
                eyre::bail!("No subdomains defined in config");
            }
            items.sort();
            items.dedup();
            select_item(&items, |s: &String| s.clone(), "Select subdomain")?
                .ok_or_else(|| eyre::eyre!("No subdomain selected — pass -s <name> to provide one"))
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

#[derive(Serialize)]
struct DnsDeleteResult {
    deleted: bool,
    fqdn: String,
    production: bool,
}

pub async fn run_dns_delete(
    subdomain: Option<String>,
    dry_run: bool,
    output: OutputFormat,
    production: bool,
    yes: bool,
) -> Result<()> {
    let subdomain = resolve_subdomain(subdomain)?;
    let service = DnsService::new_with_production(Some(production)).await?;
    let fqdn = format!("{}.{}", subdomain, service.domain());

    if dry_run {
        match output {
            OutputFormat::Json => {
                let result = DnsDeleteResult {
                    deleted: false,
                    fqdn,
                    production,
                };
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            OutputFormat::Human => {
                print_mode_banner();
                output::info(&format!("[DRY RUN] Would delete A record: {}", fqdn));
            }
        }
        return Ok(());
    }

    let confirmed = if production {
        crate::prompt::confirm_typed(
            &format!("Type '{}' to confirm production deletion", subdomain),
            &subdomain,
            yes,
        )?
    } else {
        crate::prompt::confirm(&format!("Delete A record for {}?", fqdn), yes)
    };

    if !confirmed {
        if matches!(output, OutputFormat::Human) {
            output::info("Operation cancelled");
        }
        return Ok(());
    }

    let deleted = service.delete_a_record(&subdomain).await?;

    match output {
        OutputFormat::Json => {
            let result = DnsDeleteResult {
                deleted,
                fqdn,
                production,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Human => {
            print_mode_banner();
            if deleted {
                output::success(&format!("A record deleted: {}", fqdn));
            } else {
                output::info(&format!(
                    "No A record found for {} — nothing to delete",
                    fqdn
                ));
            }
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct MigrationRow {
    subdomain: String,
    old_ip: String,
    new_ip: String,
    success: bool,
}

pub async fn run_dns_migrate(
    ip: String,
    dry_run: bool,
    output: OutputFormat,
    production: bool,
) -> Result<()> {
    let service = DnsService::new_with_production(Some(production)).await?;
    let results = service.migrate_all(&ip, dry_run).await?;

    match output {
        OutputFormat::Json => {
            let rows: Vec<MigrationRow> = results
                .iter()
                .map(|r| MigrationRow {
                    subdomain: r.subdomain.clone(),
                    old_ip: r.old_ip.clone(),
                    new_ip: r.new_ip.clone(),
                    success: r.success,
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        OutputFormat::Human => {
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
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct SkippedRow {
    app: String,
    subdomain: String,
    reason: String,
}

#[derive(Serialize)]
struct SetAllOutput {
    created: Vec<SetAllRow>,
    skipped: Vec<SkippedRow>,
    failed: Vec<SetAllRow>,
}

#[derive(Serialize)]
struct SetAllRow {
    subdomain: String,
    fqdn: String,
    ip: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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
    output: OutputFormat,
    continue_on_error: bool,
    production: bool,
) -> Result<()> {
    use crate::playbook_meta::PlaybookMeta;
    use crate::services::dns::{SubdomainEntry, discover_subdomains, discover_tailnet_only_subdomains};
    use crate::services::inventory::discover_hosts_with_ips;
    use std::collections::HashSet;

    let service = DnsService::new_with_production(Some(production)).await?;

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

    // Partition into "to create" (public) and "to skip" (tailnet-only).
    //
    // Explicit --subdomains: pre-validate that none of the named apps are
    // tailnet-only — creating public Cloudflare A records for them violates
    // ADR-0003.  Hard-error before any API call so operators learn fast.
    //
    // Implicit discovery: tailnet-only apps are already absent from
    // `discover_subdomains()`, so we collect them separately purely to emit
    // an informational skip line in the confirmation output.
    let (mut subdomains_to_process, to_skip): (Vec<(String, SubdomainEntry)>, Vec<SkippedRow>) =
        if !subdomains.is_empty() {
            let mut tailnet_only_offenders: Vec<(String, String)> = Vec::new();
            for s in &subdomains {
                if skip_set.contains(s) {
                    continue;
                }
                if let Some(meta) = PlaybookMeta::load_for_app(s)?
                    && meta.tailnet_only
                {
                    let effective_subdomain = meta.effective_subdomain(s);
                    tailnet_only_offenders.push((s.clone(), effective_subdomain));
                }
            }
            if !tailnet_only_offenders.is_empty() {
                let apps_list = tailnet_only_offenders
                    .iter()
                    .map(|(app, sub)| format!("  • {} (subdomain: {})", app, sub))
                    .collect::<Vec<_>>()
                    .join("\n");
                eyre::bail!(
                    "tailnet-only apps cannot have Cloudflare A records (ADR-0003):\n{}\n\n\
                     DNS for tailnet-only apps is published via Blocky on `auberge deploy <app>`.",
                    apps_list
                );
            }
            let to_process: Vec<(String, SubdomainEntry)> = subdomains
                .into_iter()
                .filter(|s| !skip_set.contains(s))
                .filter_map(|s| discovered.remove(&s).map(|entry| (s, entry)))
                .collect();
            (to_process, vec![])
        } else {
            let tailnet_only_discovered = discover_tailnet_only_subdomains();
            let mut skip_vec: Vec<SkippedRow> = tailnet_only_discovered
                .into_iter()
                .filter(|(k, _)| !skip_set.contains(k))
                .map(|(app, entry)| SkippedRow {
                    app,
                    subdomain: entry.subdomain,
                    reason: "tailnet_only".to_string(),
                })
                .collect();
            skip_vec.sort_by(|a, b| a.app.cmp(&b.app));

            let to_process: Vec<(String, SubdomainEntry)> = discovered
                .into_iter()
                .filter(|(k, _)| !skip_set.contains(k))
                .collect();
            (to_process, skip_vec)
        };

    if subdomains_to_process.is_empty() {
        let message = if to_skip.is_empty() {
            "No subdomains to process"
        } else {
            "All discovered apps are tailnet-only; nothing to create."
        };
        match output {
            OutputFormat::Json => {
                let result = SetAllOutput {
                    created: vec![],
                    skipped: to_skip,
                    failed: vec![],
                };
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            OutputFormat::Human => output::info(message),
        }
        return Ok(());
    }

    // Sort `subdomains_to_process` for deterministic output (`to_skip` is
    // already sorted within the implicit-discovery branch above, since it is
    // built in one place).  Both sorts happen before any output so callers
    // always see alphabetical order regardless of HashMap iteration order.
    subdomains_to_process.sort_by(|(a, _), (b, _)| a.cmp(b));

    if matches!(output, OutputFormat::Human) {
        print_mode_banner();
        if to_skip.is_empty() {
            if dry_run {
                output::info(&format!(
                    "DRY RUN - Would create {} A record(s):",
                    subdomains_to_process.len()
                ));
            } else {
                output::info(&format!(
                    "Creating {} A record(s):",
                    subdomains_to_process.len()
                ));
            }
        } else if dry_run {
            output::info(&format!(
                "DRY RUN - Would create {} A record(s), skipping {} (tailnet-only):",
                subdomains_to_process.len(),
                to_skip.len()
            ));
        } else {
            output::info(&format!(
                "Creating {} A record(s), skipping {} (tailnet-only):",
                subdomains_to_process.len(),
                to_skip.len()
            ));
        }

        eprintln!("\nTo create:");
        for (_, entry) in &subdomains_to_process {
            let effective_ip = entry.ip_override.as_deref().unwrap_or(&target_ip);
            eprintln!(
                "  • {}.{} → {}",
                entry.subdomain,
                service.domain(),
                effective_ip
            );
        }

        if !to_skip.is_empty() {
            let names: Vec<&str> = to_skip.iter().map(|r| r.app.as_str()).collect();
            eprintln!(
                "\nSkipping (tailnet-only — published via Blocky):\n  • {}",
                names.join(", ")
            );
        }
    }

    if !dry_run && !crate::prompt::confirm("Proceed?", yes) {
        if matches!(output, OutputFormat::Human) {
            output::info("Operation cancelled");
        }
        return Ok(());
    }

    if dry_run {
        if matches!(output, OutputFormat::Human) {
            output::info("DRY RUN - No changes were made");
        }
        return Ok(());
    }

    let mut created_rows: Vec<SetAllRow> = Vec::new();
    let mut failed_rows: Vec<SetAllRow> = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    if matches!(output, OutputFormat::Human) {
        eprintln!();
    }

    for (idx, (_app_name, entry)) in subdomains_to_process.iter().enumerate() {
        let effective_ip = entry.ip_override.as_deref().unwrap_or(&target_ip);
        let fqdn = format!("{}.{}", entry.subdomain, service.domain());
        match service.set_a_record(&entry.subdomain, effective_ip).await {
            Ok(_) => {
                if matches!(output, OutputFormat::Human) {
                    output::success(&format!("Created {}", fqdn));
                }
                created_rows.push(SetAllRow {
                    subdomain: entry.subdomain.clone(),
                    fqdn,
                    ip: effective_ip.to_string(),
                    success: true,
                    error: None,
                });
                succeeded += 1;
            }
            Err(e) => {
                if matches!(output, OutputFormat::Human) {
                    eprintln!("Failed {}: {}", fqdn, e);
                }
                failed_rows.push(SetAllRow {
                    subdomain: entry.subdomain.clone(),
                    fqdn,
                    ip: effective_ip.to_string(),
                    success: false,
                    error: Some(e.to_string()),
                });
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

    match output {
        OutputFormat::Json => {
            let result = SetAllOutput {
                created: created_rows,
                skipped: to_skip,
                failed: failed_rows,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Human => {
            if to_skip.is_empty() {
                output::success(&format!(
                    "Successfully created {}/{} A records pointing to {}",
                    succeeded,
                    subdomains_to_process.len(),
                    target_ip
                ));
            } else {
                output::success(&format!(
                    "Successfully created {}/{} A records pointing to {} (skipped {} tailnet-only)",
                    succeeded,
                    subdomains_to_process.len(),
                    target_ip,
                    to_skip.len()
                ));
            }
        }
    }

    if failed > 0 {
        eprintln!("Failed to create {} records", failed);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DnsDeleteResult, DnsRecordRow, DnsStatusJson, MigrationRow, SetAllOutput, SetAllRow,
        SkippedRow, StatusARecord,
    };
    #[test]
    fn dns_record_row_serialises_to_json() {
        let row = DnsRecordRow {
            name: "freshrss.example.com".to_string(),
            record_type: "A".to_string(),
            content: "192.168.1.10".to_string(),
            ttl: 1,
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"name\":\"freshrss.example.com\""));
        assert!(json.contains("\"record_type\":\"A\""));
        assert!(json.contains("\"ttl\":1"));
    }

    #[test]
    fn dns_status_json_serialises_with_nested_records() {
        let status = DnsStatusJson {
            domain: "example.com".to_string(),
            configured_subdomains: vec!["freshrss".to_string()],
            active_a_records: vec![StatusARecord {
                name: "freshrss.example.com".to_string(),
                ip: "192.168.1.10".to_string(),
            }],
            missing_subdomains: vec![],
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"missing_subdomains\":[]"));
        assert!(json.contains("\"active_a_records\":[{\"name\":\"freshrss.example.com\""));
    }

    #[test]
    fn migration_row_serialises_with_success_flag() {
        let row = MigrationRow {
            subdomain: "baikal".to_string(),
            old_ip: "1.2.3.4".to_string(),
            new_ip: "5.6.7.8".to_string(),
            success: true,
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn set_all_row_omits_error_field_when_success() {
        let row = SetAllRow {
            subdomain: "baikal".to_string(),
            fqdn: "baikal.example.com".to_string(),
            ip: "1.2.3.4".to_string(),
            success: true,
            error: None,
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn set_all_row_includes_error_field_on_failure() {
        let row = SetAllRow {
            subdomain: "baikal".to_string(),
            fqdn: "baikal.example.com".to_string(),
            ip: "1.2.3.4".to_string(),
            success: false,
            error: Some("timeout".to_string()),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"error\":\"timeout\""));
        assert!(json.contains("\"success\":false"));
    }

    // The `deleted` field is what makes `dns delete` a load-bearing-JSON
    // command (vs. `dns set` which only echoes input). Lock both branches
    // under test so a future refactor that drops the field surfaces here.
    #[test]
    fn dns_delete_result_distinguishes_real_delete_from_noop() {
        let real = DnsDeleteResult {
            deleted: true,
            fqdn: "freshrss.example.com".to_string(),
            production: false,
        };
        let noop = DnsDeleteResult {
            deleted: false,
            fqdn: "freshrss.example.com".to_string(),
            production: false,
        };
        assert!(
            serde_json::to_string(&real)
                .unwrap()
                .contains("\"deleted\":true")
        );
        assert!(
            serde_json::to_string(&noop)
                .unwrap()
                .contains("\"deleted\":false")
        );
    }

    // Lock the `SetAllOutput` JSON shape: top-level object with `created`,
    // `skipped`, and `failed` arrays, parallel to
    // `dns_delete_result_distinguishes_real_delete_from_noop`.
    #[test]
    fn set_all_output_serialises_with_created_skipped_failed_arrays() {
        let output = SetAllOutput {
            created: vec![SetAllRow {
                subdomain: "rss".to_string(),
                fqdn: "rss.example.com".to_string(),
                ip: "1.2.3.4".to_string(),
                success: true,
                error: None,
            }],
            skipped: vec![SkippedRow {
                app: "bichon".to_string(),
                subdomain: "bichon".to_string(),
                reason: "tailnet_only".to_string(),
            }],
            failed: vec![],
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"created\":[{"));
        assert!(json.contains("\"skipped\":[{"));
        assert!(json.contains("\"failed\":[]"));
        assert!(json.contains("\"reason\":\"tailnet_only\""));
        assert!(json.contains("\"app\":\"bichon\""));
        assert!(json.contains("\"subdomain\":\"bichon\""));
    }

    #[test]
    fn set_all_output_all_tailnet_only_produces_empty_created_and_failed() {
        let output = SetAllOutput {
            created: vec![],
            skipped: vec![
                SkippedRow {
                    app: "bichon".to_string(),
                    subdomain: "bichon".to_string(),
                    reason: "tailnet_only".to_string(),
                },
                SkippedRow {
                    app: "paperless".to_string(),
                    subdomain: "paperless".to_string(),
                    reason: "tailnet_only".to_string(),
                },
            ],
            failed: vec![],
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"created\":[]"));
        assert!(json.contains("\"failed\":[]"));
        assert!(json.contains("\"bichon\""));
        assert!(json.contains("\"paperless\""));
    }

    #[test]
    fn skipped_row_serialises_with_app_subdomain_reason() {
        let row = SkippedRow {
            app: "cockpit".to_string(),
            subdomain: "cockpit".to_string(),
            reason: "tailnet_only".to_string(),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"app\":\"cockpit\""));
        assert!(json.contains("\"subdomain\":\"cockpit\""));
        assert!(json.contains("\"reason\":\"tailnet_only\""));
    }

    #[test]
    fn discover_tailnet_only_subdomains_returns_tailnet_apps() {
        use crate::services::dns::discover_tailnet_only_subdomains;
        let discovered = discover_tailnet_only_subdomains();
        for app in ["bichon", "cockpit", "paperless"] {
            assert!(
                discovered.contains_key(app),
                "tailnet-only app '{app}' must appear in tailnet-only discovery"
            );
        }
    }

    #[test]
    fn discover_tailnet_only_subdomains_excludes_public_apps() {
        use crate::services::dns::discover_tailnet_only_subdomains;
        let discovered = discover_tailnet_only_subdomains();
        for app in ["freshrss", "baikal", "navidrome"] {
            assert!(
                !discovered.contains_key(app),
                "public app '{app}' must not appear in tailnet-only discovery"
            );
        }
    }

    #[test]
    fn explicit_subdomains_containing_tailnet_only_app_errors() {
        use crate::playbook_meta::PlaybookMeta;
        // Verify that bichon is indeed tailnet_only so the hard-error path
        // is exercisable from the meta files in this repo.
        let meta = PlaybookMeta::load_for_app("bichon")
            .expect("load should not fail")
            .expect("bichon meta should exist");
        assert!(
            meta.tailnet_only,
            "bichon must be tailnet_only for the explicit-subdomains hard-error path"
        );
    }

    #[test]
    fn explicit_subdomains_tailnet_only_error_surfaces_effective_subdomain() {
        use crate::playbook_meta::PlaybookMeta;
        // When a tailnet-only app has a subdomain override in its meta, the
        // error message must use the effective subdomain, not the app name,
        // so operators who configured overrides are not confused.
        let meta = PlaybookMeta::load_for_app("paperless")
            .expect("load should not fail")
            .expect("paperless meta should exist");
        assert!(meta.tailnet_only);
        // The effective subdomain is what would appear in the error message.
        let effective = meta.effective_subdomain("paperless");
        assert!(
            !effective.is_empty(),
            "tailnet-only app must declare a subdomain for effective-subdomain surfacing"
        );
    }
}

