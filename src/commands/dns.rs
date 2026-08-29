use crate::output;
use crate::output::OutputFormat;
use crate::prompt::{Choice, select_item};
use crate::services::cloudflare_dns::CloudflareDns;
use crate::services::dns::{
    AppliedRecord, DiscoveredSubdomains, DnsRecords, MigrationOutcome, PlannedRecord,
    SetAllOutcome, SetAllPlan, WRITE_PACE, apply_set_all, discover_all_subdomains, plan_set_all,
};
use clap::Subcommand;
use dialoguer::{Input, theme::ColorfulTheme};
use eyre::Result;
use serde::Serialize;

#[derive(Subcommand)]
pub enum DnsCommands {
    #[command(visible_alias = "l", about = "List DNS records")]
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
    },
    #[command(visible_alias = "st", about = "Show DNS status and health")]
    Status {
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
    },
    #[command(visible_alias = "s", about = "Set an A record for a subdomain")]
    Set {
        #[arg(short, long, help = "Subdomain name")]
        subdomain: Option<String>,
        #[arg(short, long, help = "IP address")]
        ip: Option<String>,
    },
    #[command(
        visible_alias = "d",
        about = "Delete an A record for a subdomain",
        long_about = "Delete the Cloudflare A record for a subdomain.\n\n\
                      Idempotent — running against an already-absent record reports success. \
                      Only A records are considered; CNAME / AAAA / TXT records for the same \
                      name are left untouched.\n\n\
                      Confirmation is required by default; --yes skips it. Production deletions \
                      escalate the confirmation: the user must retype the subdomain name to \
                      proceed.\n\n\
                      EXAMPLES:\n  \
                      # Pick a subdomain interactively, confirm, then delete\n  \
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
        #[arg(
            short = 'P',
            long,
            help = "Treat as a production deletion: retype the subdomain to confirm"
        )]
        production: bool,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
    },
    #[command(visible_alias = "m", about = "Migrate all A records to a new IP")]
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
    },
    #[command(
        visible_alias = "sa",
        about = "Batch create A records for all app subdomains",
        long_about = "Interactively or automatically create DNS A records for all configured \
                      app subdomains pointing to a selected host's IP address.\n\n\
                      Tailnet-only apps (playbook meta `tailnet_only: true`) are handled \
                      automatically per ADR-0003:\n\n\
                      • Implicit discovery (no --subdomains): tailnet-only apps are skipped \
                        automatically; a grouped info line is emitted to stderr.\n\
                      • Explicit target (--subdomains names a tailnet-only app): hard-error \
                        before any record is written; use `auberge deploy <app>` instead.\n\n\
                      EXAMPLES:\n  \
                      # Publish all Public Apps; tailnet-only apps are skipped automatically\n  \
                      auberge dns set-all --host auberge\n\n  \
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
            help = "Target host (omit to be prompted)"
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

#[derive(Serialize)]
struct DnsRecordRow {
    name: String,
    record_type: String,
    content: String,
    ttl: u32,
}

fn print_mode_banner() {
    output::info("CLOUDFLARE DNS");
}

pub async fn run_dns_list(subdomain: Option<String>, output: OutputFormat) -> Result<()> {
    let dns = CloudflareDns::connect().await?;
    let records = dns.list_records().await?;

    let filtered: Vec<_> = match &subdomain {
        Some(name) => records.iter().filter(|r| r.name == *name).collect(),
        None => records.iter().collect(),
    };

    match output {
        OutputFormat::Json => {
            let rows: Vec<DnsRecordRow> = filtered
                .iter()
                .map(|r| DnsRecordRow {
                    name: r.name.clone(),
                    record_type: r.content.kind().to_string(),
                    content: r.content.value(),
                    ttl: r.ttl,
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
                dns.domain(),
                "NAME",
                "TYPE",
                "CONTENT",
                "TTL"
            );
            eprintln!("{}", "-".repeat(80));
            for record in filtered {
                eprintln!(
                    "{:<40} {:<8} {:<24} {:>6}",
                    record.name,
                    record.content.kind(),
                    record.content.value(),
                    record.ttl
                );
            }
        }
    }

    Ok(())
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

/// The subdomains `dns status` expects to find published, sorted so the report
/// does not inherit `HashMap` iteration order.
fn configured_subdomains() -> Vec<String> {
    let mut names: Vec<String> = crate::services::dns::discover_subdomains()
        .into_values()
        .map(|e| e.subdomain)
        .collect();
    names.sort();
    names
}

pub async fn run_dns_status(output: OutputFormat) -> Result<()> {
    let dns = CloudflareDns::connect().await?;
    let status = crate::services::dns::status(&dns, configured_subdomains()).await?;

    let a_records: Vec<(&str, String)> = status
        .active_records
        .iter()
        .filter_map(|r| r.a_ip().map(|ip| (r.name.as_str(), ip.to_string())))
        .collect();

    match output {
        OutputFormat::Json => {
            let json_status = DnsStatusJson {
                domain: status.domain.clone(),
                configured_subdomains: status.configured_subdomains.clone(),
                active_a_records: a_records
                    .iter()
                    .map(|(name, ip)| StatusARecord {
                        name: (*name).to_string(),
                        ip: ip.clone(),
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
            for (name, ip) in &a_records {
                eprintln!("  {} -> {}", name, ip);
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
            let mut items = configured_subdomains();
            if items.is_empty() {
                eyre::bail!("No subdomains defined in config");
            }
            items.dedup();
            select_item(
                &items,
                |s: &String| s.clone(),
                Choice::new("subdomain").resolved_by("-s <name>"),
            )
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

pub async fn run_dns_set(subdomain: Option<String>, ip: Option<String>) -> Result<()> {
    let subdomain = resolve_subdomain(subdomain)?;
    let ip = resolve_ip(ip)?;

    let dns = CloudflareDns::connect().await?;
    print_mode_banner();

    output::info(&format!(
        "Setting A record: {}.{} -> {}",
        subdomain,
        dns.domain(),
        ip
    ));

    dns.set_a_record(&subdomain, &ip).await?;
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
    let dns = CloudflareDns::connect().await?;
    let fqdn = format!("{}.{}", subdomain, dns.domain());

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

    let deleted = dns.delete_a_record(&subdomain).await?;

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

/// One `SkippedRecord` as the JSON body spells it.
#[derive(Serialize)]
struct MigrationSkippedRow {
    subdomain: String,
    ip: String,
    reason: String,
}

/// The ADR-0004 body, mirroring `MigrationOutcome`.
#[derive(Serialize)]
struct MigrationOutput {
    migrated: Vec<MigrationRow>,
    skipped: Vec<MigrationSkippedRow>,
}

fn migration_json(outcome: &MigrationOutcome) -> MigrationOutput {
    MigrationOutput {
        migrated: outcome
            .migrated
            .iter()
            .map(|r| MigrationRow {
                subdomain: r.subdomain.clone(),
                old_ip: r.old_ip.clone(),
                new_ip: r.new_ip.clone(),
                success: r.success,
            })
            .collect(),
        skipped: outcome
            .skipped
            .iter()
            .map(|s| MigrationSkippedRow {
                subdomain: s.subdomain.clone(),
                ip: s.ip.clone(),
                reason: s.reason.as_str().to_string(),
            })
            .collect(),
    }
}

fn print_migration(outcome: &MigrationOutcome, dry_run: bool) {
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
    for result in &outcome.migrated {
        eprintln!(
            "{:<14} {:<16} ->  {:<16}",
            result.subdomain, result.old_ip, result.new_ip
        );
    }

    if !outcome.skipped.is_empty() {
        eprintln!("\nSkipping (holds a tailnet address, not a Host address):");
        for skipped in &outcome.skipped {
            eprintln!("  • {} ({})", skipped.subdomain, skipped.ip);
        }
    }

    let skipped = tailnet_only_suffix(outcome.skipped.len());
    if dry_run {
        eprintln!(
            "\nWould update {} A record(s).{}",
            outcome.migrated.len(),
            skipped
        );
    } else {
        let success_count = outcome.migrated.iter().filter(|r| r.success).count();
        eprintln!("\nUpdated {} A record(s).{}", success_count, skipped);
    }
}

pub async fn run_dns_migrate(ip: String, dry_run: bool, output: OutputFormat) -> Result<()> {
    let dns = CloudflareDns::connect().await?;
    let outcome = crate::services::dns::migrate_all(&dns, &ip, dry_run).await?;

    match output {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&migration_json(&outcome))?
        ),
        OutputFormat::Human => print_migration(&outcome, dry_run),
    }

    Ok(())
}

#[derive(Serialize, Debug)]
struct SkippedRow {
    app: String,
    subdomain: String,
    reason: String,
}

/// What the run did with its plan. The one field an ADR-0004 consumer can
/// branch on without reading stderr: `created` is empty on a dry run, a
/// cancelled run, and an all-failed run alike, so emptiness decides nothing.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RunOutcome {
    Applied,
    DryRun,
    Cancelled,
}

/// The outcome an empty plan reports. Nothing runs and nothing is confirmed,
/// so the only distinction left is whether the operator asked for a preview:
/// `cancelled` is unreachable here because there is nothing to proceed with.
fn empty_plan_outcome(dry_run: bool) -> RunOutcome {
    if dry_run {
        RunOutcome::DryRun
    } else {
        RunOutcome::Applied
    }
}

#[derive(Serialize)]
struct PlannedRow {
    app: String,
    subdomain: String,
    fqdn: String,
    ip: String,
}

impl PlannedRow {
    fn from(planned: &PlannedRecord) -> Self {
        Self {
            app: planned.app.clone(),
            subdomain: planned.subdomain.clone(),
            fqdn: planned.fqdn.clone(),
            ip: planned.ip.clone(),
        }
    }
}

/// `planned` holds the full plan on every outcome — the denominator the other
/// arrays are read against. On an applied run `created` and `failed` partition
/// it; on a dry run or a cancelled run it is the primary data, and the records
/// it holds appear nowhere else because nothing was attempted.
#[derive(Serialize)]
struct SetAllOutput {
    outcome: RunOutcome,
    planned: Vec<PlannedRow>,
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

/// The `error` a record the run never reached carries. Not a provider message
/// — the reason no provider was asked.
const NOT_ATTEMPTED: &str =
    "not attempted: an earlier record failed and --continue-on-error is off";

impl SetAllRow {
    fn from(applied: &AppliedRecord) -> Self {
        Self {
            subdomain: applied.subdomain.clone(),
            fqdn: applied.fqdn.clone(),
            ip: applied.ip.clone(),
            success: applied.error.is_none(),
            error: applied.error.clone(),
        }
    }

    fn not_attempted(planned: &PlannedRecord) -> Self {
        Self {
            subdomain: planned.subdomain.clone(),
            fqdn: planned.fqdn.clone(),
            ip: planned.ip.clone(),
            success: false,
            error: Some(NOT_ATTEMPTED.to_string()),
        }
    }
}

/// The one stdout write `--output json` makes, whichever path the run ends on
/// (ADR-0044).
fn print_set_all_body(plan: &SetAllPlan, applied: &SetAllOutcome, run: RunOutcome) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&set_all_json(plan, applied, run))?
    );
    Ok(())
}

fn set_all_json(plan: &SetAllPlan, applied: &SetAllOutcome, run: RunOutcome) -> SetAllOutput {
    SetAllOutput {
        outcome: run,
        planned: plan.to_create.iter().map(PlannedRow::from).collect(),
        created: applied.created.iter().map(SetAllRow::from).collect(),
        skipped: plan
            .skipped
            .iter()
            .map(|s| SkippedRow {
                app: s.app.clone(),
                subdomain: s.subdomain.clone(),
                reason: s.reason.as_str().to_string(),
            })
            .collect(),
        failed: applied
            .failed
            .iter()
            .map(SetAllRow::from)
            .chain(applied.not_attempted.iter().map(SetAllRow::not_attempted))
            .collect(),
    }
}

fn print_plan(plan: &SetAllPlan, dry_run: bool) {
    let verb = if dry_run {
        "DRY RUN - Would create"
    } else {
        "Creating"
    };
    if plan.skipped.is_empty() {
        output::info(&format!("{} {} A record(s):", verb, plan.to_create.len()));
    } else {
        output::info(&format!(
            "{} {} A record(s), skipping {} (tailnet-only):",
            verb,
            plan.to_create.len(),
            plan.skipped.len()
        ));
    }

    eprintln!("\nTo create:");
    for record in &plan.to_create {
        eprintln!("  • {} → {}", record.fqdn, record.ip);
    }

    if !plan.skipped.is_empty() {
        let names: Vec<&str> = plan.skipped.iter().map(|s| s.app.as_str()).collect();
        eprintln!(
            "\nSkipping (tailnet-only — published via Blocky):\n  • {}",
            names.join(", ")
        );
    }
}

/// The tail both closing lines carry, empty when nothing was skipped.
fn tailnet_only_suffix(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!(" (skipped {count} tailnet-only)")
    }
}

/// The closing line. A run with failures does not get a success banner, and
/// says how many records it abandoned rather than implying it tried them: the
/// denominator is the plan, and only `created` is a claim about what landed.
fn print_summary(plan: &SetAllPlan, outcome: &SetAllOutcome, target_ip: &str) {
    let skipped = tailnet_only_suffix(plan.skipped.len());

    if outcome.failed.is_empty() {
        output::success(&format!(
            "Successfully created {}/{} A records pointing to {}{}",
            outcome.created.len(),
            plan.to_create.len(),
            target_ip,
            skipped
        ));
        return;
    }

    let abandoned = if outcome.not_attempted.is_empty() {
        String::new()
    } else {
        format!(", {} not attempted", outcome.not_attempted.len())
    };
    output::warn(&format!(
        "Created {}/{} A records pointing to {}; {} failed{}{}",
        outcome.created.len(),
        plan.to_create.len(),
        target_ip,
        outcome.failed.len(),
        abandoned,
        skipped
    ));
}

fn resolve_target_ip(host: Option<String>, ip: Option<String>, strict: bool) -> Result<String> {
    use crate::services::inventory::discover_hosts_with_ips;
    match (host, ip) {
        (None, Some(ip_addr)) => Ok(ip_addr),
        (None, None) if strict => {
            eyre::bail!("Either --host or --ip must be specified in strict mode")
        }
        (host, None) => target_ip_from_inventory(host, discover_hosts_with_ips(None)?),
        (Some(_), Some(_)) => unreachable!("clap declares --ip conflicts_with --host"),
    }
}

/// The Inventory half of resolving `set-all`'s one logical input, the address
/// records point at: an explicit Host name is looked up, no name prompts over
/// the Inventory (#704). `--ip` never reaches here — it is the escape hatch
/// for an address not in the Inventory, resolved before the Inventory is read.
fn target_ip_from_inventory(
    host: Option<String>,
    hosts: std::collections::HashMap<String, String>,
) -> Result<String> {
    match host {
        Some(host_name) => hosts.get(&host_name).cloned().ok_or_else(|| {
            let mut available: Vec<&str> = hosts.keys().map(String::as_str).collect();
            available.sort_unstable();
            eyre::eyre!(
                "Host '{}' not found in inventory. Available: {}",
                host_name,
                available.join(", ")
            )
        }),
        None => {
            let mut candidates: Vec<(String, String)> = hosts.into_iter().collect();
            candidates.sort();
            let (_, ip) = select_item(
                &candidates,
                |(name, ip): &(String, String)| format!("{} ({})", name, ip),
                crate::hosts::host_choice(&format!("{} (or -i <ip>)", crate::hosts::HOST_FLAG)),
            )?;
            Ok(ip)
        }
    }
}

pub struct SetAllOptions {
    pub host: Option<String>,
    pub ip: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
    pub strict: bool,
    pub subdomains: Vec<String>,
    pub skip: Vec<String>,
    pub output: OutputFormat,
    pub continue_on_error: bool,
}

/// Returns the process exit code — the Backup Verdict convention `backup
/// verify` and `versions` already use (0 every record written, 1 at least one
/// write failed, 2 operational error), so a caller can branch on which of the
/// three happened instead of parsing the summary.
pub async fn run_dns_set_all(opts: SetAllOptions) -> i32 {
    set_all_exit_code(connect_and_set_all(opts).await)
}

/// The two things `set_all` needs from the outside world, resolved once so the
/// orchestration below runs against a fake.
async fn connect_and_set_all(opts: SetAllOptions) -> Result<SetAllOutcome> {
    let dns = CloudflareDns::connect().await?;
    let discovered = discover_all_subdomains();
    set_all(&dns, discovered, opts).await
}

fn set_all_exit_code(result: Result<SetAllOutcome>) -> i32 {
    match result {
        Ok(outcome) => {
            if outcome.failed.is_empty() {
                0
            } else {
                eprintln!("Failed to create {} records", outcome.failed.len());
                1
            }
        }
        Err(e) => {
            eprintln!("✗ {e:#}");
            2
        }
    }
}

async fn set_all<D: DnsRecords>(
    dns: &D,
    discovered: DiscoveredSubdomains,
    opts: SetAllOptions,
) -> Result<SetAllOutcome> {
    use std::collections::HashSet;

    let SetAllOptions {
        host,
        ip,
        dry_run,
        yes,
        strict,
        subdomains,
        skip,
        output,
        continue_on_error,
    } = opts;

    let target_ip = resolve_target_ip(host, ip, strict)?;

    if strict && discovered.public.is_empty() {
        eyre::bail!("No subdomain environment variables found");
    }

    let skip_set: HashSet<String> = skip.into_iter().collect();
    let plan = plan_set_all(dns.domain(), &target_ip, discovered, subdomains, &skip_set)?;
    let human = matches!(output, OutputFormat::Human);

    if plan.to_create.is_empty() {
        if human {
            output::info(if plan.skipped.is_empty() {
                "No subdomains to process"
            } else {
                "All discovered apps are tailnet-only; nothing to create."
            });
        } else {
            print_set_all_body(
                &plan,
                &SetAllOutcome::default(),
                empty_plan_outcome(dry_run),
            )?;
        }
        return Ok(SetAllOutcome::default());
    }

    if human {
        print_mode_banner();
        print_plan(&plan, dry_run);
    }

    if dry_run {
        if human {
            output::info("DRY RUN - No changes were made");
        } else {
            print_set_all_body(&plan, &SetAllOutcome::default(), RunOutcome::DryRun)?;
        }
        return Ok(SetAllOutcome::default());
    }

    if !crate::prompt::confirm("Proceed?", yes) {
        if human {
            output::info("Operation cancelled");
        } else {
            print_set_all_body(&plan, &SetAllOutcome::default(), RunOutcome::Cancelled)?;
        }
        return Ok(SetAllOutcome::default());
    }

    if human {
        eprintln!();
    }
    let mut report = |applied: &AppliedRecord| {
        if !human {
            return;
        }
        match &applied.error {
            None => output::success(&format!("Created {}", applied.fqdn)),
            Some(message) => eprintln!("Failed {}: {}", applied.fqdn, message),
        }
    };
    let outcome = apply_set_all(dns, &plan, continue_on_error, WRITE_PACE, &mut report).await;

    if human {
        print_summary(&plan, &outcome, &target_ip);
    } else {
        print_set_all_body(&plan, &outcome, RunOutcome::Applied)?;
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::dns::{
        MigrationResult, PlannedRecord, SkipReason, SkippedApp, SkippedRecord,
    };
    use clap::Parser;

    #[derive(clap::Parser)]
    struct DnsCli {
        #[command(subcommand)]
        cmd: DnsCommands,
    }

    // The one subcommand where --production is load-bearing: it escalates the
    // confirmation to a retyped subdomain and lands in the JSON body.
    #[test]
    fn delete_keeps_the_production_flag() {
        let cli = DnsCli::try_parse_from(["dns", "delete", "-s", "freshrss", "--production"])
            .expect("dns delete must keep accepting --production");
        match cli.cmd {
            DnsCommands::Delete { production, .. } => assert!(production),
            _ => unreachable!("parsed 'delete' into another variant"),
        }
    }

    // A script passing the flag must fail loudly rather than read as if it
    // had chosen an environment: every call uses the production API.
    #[test]
    fn production_is_an_error_on_the_subcommands_that_never_read_it() {
        let invocations: [&[&str]; 5] = [
            &["dns", "list"],
            &["dns", "status"],
            &["dns", "set", "-s", "freshrss", "-i", "1.2.3.4"],
            &["dns", "migrate", "-i", "1.2.3.4"],
            &["dns", "set-all", "--host", "auberge"],
        ];
        for prefix in invocations {
            for flag in ["--production", "-P"] {
                let args = [prefix, &[flag]].concat();
                let Err(err) = DnsCli::try_parse_from(&args) else {
                    panic!("{:?} must be rejected", args.join(" "));
                };
                assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
            }
        }
    }

    fn applied(subdomain: &str, error: Option<&str>) -> AppliedRecord {
        AppliedRecord {
            subdomain: subdomain.to_string(),
            fqdn: format!("{subdomain}.example.com"),
            ip: "1.2.3.4".to_string(),
            error: error.map(str::to_string),
        }
    }

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

    fn migrated(subdomain: &str) -> MigrationResult {
        MigrationResult {
            subdomain: subdomain.to_string(),
            old_ip: "203.0.113.10".to_string(),
            new_ip: "198.51.100.7".to_string(),
            success: true,
        }
    }

    fn skipped_record(subdomain: &str, ip: &str) -> SkippedRecord {
        SkippedRecord {
            subdomain: subdomain.to_string(),
            ip: ip.to_string(),
            reason: SkipReason::TailnetOnly,
        }
    }

    // `migrate -o json` answers the ADR-0003 skip as data, in `set-all`'s
    // `skipped` shape: one row per record, `reason` named rather than implied.
    #[test]
    fn migration_output_serialises_with_migrated_and_skipped_arrays() {
        let outcome = MigrationOutcome {
            migrated: vec![migrated("rss")],
            skipped: vec![skipped_record("bichon", "100.64.0.9")],
        };
        let json = serde_json::to_string(&migration_json(&outcome)).unwrap();
        assert!(json.contains("\"migrated\":[{"));
        assert!(json.contains("\"skipped\":[{"));
        assert!(json.contains("\"subdomain\":\"bichon\""));
        assert!(json.contains("\"ip\":\"100.64.0.9\""));
        assert!(json.contains("\"reason\":\"tailnet_only\""));
    }

    // A zone of nothing but tailnet-only records still emits a body, so a
    // consumer never has to read an empty array as "the run saw nothing".
    #[test]
    fn migration_output_over_a_tailnet_only_zone_carries_only_skips() {
        let outcome = MigrationOutcome {
            migrated: vec![],
            skipped: vec![
                skipped_record("bichon", "100.64.0.9"),
                skipped_record("cockpit", "100.101.255.46"),
            ],
        };
        let json = serde_json::to_string(&migration_json(&outcome)).unwrap();
        assert!(json.contains("\"migrated\":[]"));
        assert_eq!(migration_json(&outcome).skipped.len(), 2);
    }

    #[test]
    fn set_all_row_omits_error_field_when_success() {
        let json = serde_json::to_string(&SetAllRow::from(&applied("baikal", None))).unwrap();
        assert!(!json.contains("\"error\""));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn set_all_row_includes_error_field_on_failure() {
        let json =
            serde_json::to_string(&SetAllRow::from(&applied("baikal", Some("timeout")))).unwrap();
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

    fn plan_with(to_create: &[&str], skipped: &[&str]) -> SetAllPlan {
        SetAllPlan {
            to_create: to_create
                .iter()
                .map(|app| PlannedRecord {
                    app: (*app).to_string(),
                    subdomain: (*app).to_string(),
                    fqdn: format!("{app}.example.com"),
                    ip: "1.2.3.4".to_string(),
                })
                .collect(),
            skipped: skipped
                .iter()
                .map(|app| SkippedApp {
                    app: (*app).to_string(),
                    subdomain: (*app).to_string(),
                    reason: SkipReason::TailnetOnly,
                })
                .collect(),
        }
    }

    // Lock the `SetAllOutput` JSON shape: top-level object with `created`,
    // `skipped`, and `failed` arrays (ADR-0004).
    #[test]
    fn set_all_output_serialises_with_created_skipped_failed_arrays() {
        let plan = plan_with(&["rss"], &["bichon"]);
        let outcome = SetAllOutcome {
            created: vec![applied("rss", None)],
            failed: vec![],
            not_attempted: vec![],
        };
        let json =
            serde_json::to_string(&set_all_json(&plan, &outcome, RunOutcome::Applied)).unwrap();
        assert!(json.contains("\"created\":[{"));
        assert!(json.contains("\"skipped\":[{"));
        assert!(json.contains("\"failed\":[]"));
        assert!(json.contains("\"reason\":\"tailnet_only\""));
        assert!(json.contains("\"app\":\"bichon\""));
        assert!(json.contains("\"subdomain\":\"bichon\""));
    }

    #[test]
    fn set_all_output_all_tailnet_only_produces_empty_created_and_failed() {
        let plan = plan_with(&[], &["bichon", "paperless"]);
        let json = serde_json::to_string(&set_all_json(
            &plan,
            &SetAllOutcome::default(),
            RunOutcome::Applied,
        ))
        .unwrap();
        assert!(json.contains("\"created\":[]"));
        assert!(json.contains("\"failed\":[]"));
        assert!(json.contains("\"bichon\""));
        assert!(json.contains("\"paperless\""));
    }

    // A failed write used to reach the operator only as a bare exit(1) — the
    // JSON body was never emitted. It now carries the failure.
    #[test]
    fn set_all_output_carries_the_failed_record_and_its_error() {
        let plan = plan_with(&["rss"], &[]);
        let outcome = SetAllOutcome {
            created: vec![],
            failed: vec![applied("rss", Some("cloudflare rejected rss"))],
            not_attempted: vec![],
        };
        let json =
            serde_json::to_string(&set_all_json(&plan, &outcome, RunOutcome::Applied)).unwrap();
        assert!(json.contains("\"created\":[]"));
        assert!(json.contains("\"error\":\"cloudflare rejected rss\""));
    }

    // The whole plan must be reconcilable from the body. Fail-fast stops after
    // the first failure, and a `failed` array holding only that one record
    // would leave the reader thinking the plan had one record in it.
    #[test]
    fn set_all_output_names_every_planned_record_after_a_fail_fast_stop() {
        let plan = plan_with(&["baikal", "rss", "music"], &[]);
        let outcome = SetAllOutcome {
            created: vec![],
            failed: vec![applied("baikal", Some("cloudflare rejected baikal"))],
            not_attempted: plan.to_create[1..].to_vec(),
        };
        let body = set_all_json(&plan, &outcome, RunOutcome::Applied);

        let named: Vec<&str> = body
            .created
            .iter()
            .chain(body.failed.iter())
            .map(|r| r.subdomain.as_str())
            .collect();
        assert_eq!(named, vec!["baikal", "rss", "music"]);
        assert!(body.failed.iter().all(|r| !r.success));
        assert_eq!(
            body.failed[1].error.as_deref(),
            Some(NOT_ATTEMPTED),
            "an abandoned record says why no provider was asked"
        );
        assert_eq!(
            body.failed[0].error.as_deref(),
            Some("cloudflare rejected baikal"),
            "the real failure keeps the provider's own message"
        );
    }

    // A dry run's primary data is the plan. It reaches stdout under `planned`
    // with the effective fqdn and IP per record, and `created` never holds a
    // record that was not created.
    #[test]
    fn set_all_output_dry_run_puts_the_plan_under_planned() {
        let plan = plan_with(&["baikal", "rss"], &["bichon"]);
        let json = serde_json::to_string(&set_all_json(
            &plan,
            &SetAllOutcome::default(),
            RunOutcome::DryRun,
        ))
        .unwrap();
        assert!(json.contains("\"outcome\":\"dry_run\""));
        assert!(json.contains("\"planned\":[{"));
        assert!(json.contains("\"app\":\"baikal\""));
        assert!(json.contains("\"fqdn\":\"baikal.example.com\""));
        assert!(json.contains("\"ip\":\"1.2.3.4\""));
        assert!(json.contains("\"created\":[]"));
        assert!(json.contains("\"failed\":[]"));
        assert!(json.contains("\"reason\":\"tailnet_only\""));
    }

    // An empty plan short-circuits before the prompt, so its body can only
    // report a preview or a vacuous apply — and must not label the preview
    // `applied`, which would claim a run the operator asked not to have.
    #[test]
    fn empty_plan_outcome_reports_the_preview_and_nothing_else() {
        assert_eq!(empty_plan_outcome(true), RunOutcome::DryRun);
        assert_eq!(empty_plan_outcome(false), RunOutcome::Applied);
    }

    // A declined `Proceed?` is what a non-TTY consumer without `--yes` hits:
    // confirm() refuses off-terminal. The body must say so — before this,
    // that path was empty stdout + exit 0, indistinguishable from a run with
    // nothing to do.
    #[test]
    fn set_all_output_cancelled_run_says_so_and_keeps_the_plan() {
        let plan = plan_with(&["rss"], &[]);
        let json = serde_json::to_string(&set_all_json(
            &plan,
            &SetAllOutcome::default(),
            RunOutcome::Cancelled,
        ))
        .unwrap();
        assert!(json.contains("\"outcome\":\"cancelled\""));
        assert!(json.contains("\"planned\":[{"));
        assert!(json.contains("\"fqdn\":\"rss.example.com\""));
        assert!(json.contains("\"created\":[]"));
        assert!(json.contains("\"failed\":[]"));
    }

    // `planned` is the denominator on every outcome: an applied run carries
    // the same plan its `created`/`failed` arrays partition, so a fail-fast
    // stop is reconcilable without knowing the NOT_ATTEMPTED convention.
    #[test]
    fn set_all_output_applied_run_still_carries_the_plan_it_applied() {
        let plan = plan_with(&["baikal", "rss"], &[]);
        let outcome = SetAllOutcome {
            created: vec![applied("baikal", None), applied("rss", None)],
            failed: vec![],
            not_attempted: vec![],
        };
        let body = set_all_json(&plan, &outcome, RunOutcome::Applied);
        let planned: Vec<&str> = body.planned.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(planned, vec!["baikal", "rss"]);
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"outcome\":\"applied\""));
    }

    #[test]
    fn skipped_row_serialises_with_app_subdomain_reason() {
        let row = SkippedRow {
            app: "cockpit".to_string(),
            subdomain: "cockpit".to_string(),
            reason: SkipReason::TailnetOnly.as_str().to_string(),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"app\":\"cockpit\""));
        assert!(json.contains("\"subdomain\":\"cockpit\""));
        assert!(json.contains("\"reason\":\"tailnet_only\""));
    }

    // The Backup Verdict convention, matching `versions_exit_code`: 0 every
    // record written, 1 at least one write failed, 2 operational error.
    #[test]
    fn set_all_exit_code_mirrors_the_backup_verdict_convention() {
        assert_eq!(set_all_exit_code(Ok(SetAllOutcome::default())), 0);
        assert_eq!(
            set_all_exit_code(Ok(SetAllOutcome {
                created: vec![applied("rss", None)],
                failed: vec![],
                not_attempted: vec![],
            })),
            0
        );
        assert_eq!(
            set_all_exit_code(Ok(SetAllOutcome {
                created: vec![applied("rss", None)],
                failed: vec![applied("baikal", Some("timeout"))],
                not_attempted: vec![],
            })),
            1
        );
        assert_eq!(set_all_exit_code(Err(eyre::eyre!("boom"))), 2);
    }

    // An ADR-0003 violation is refused before any write, so it is an
    // operational error (2) and never a partial-write verdict (1).
    #[test]
    fn set_all_exit_code_separates_an_operational_error_from_a_failed_write() {
        let refused = plan_set_all(
            "example.com",
            "1.2.3.4",
            crate::services::dns::DiscoveredSubdomains {
                public: Default::default(),
                tailnet_only: [(
                    "paperless".to_string(),
                    crate::services::dns::SubdomainEntry {
                        subdomain: "docs".to_string(),
                        ip_override: None,
                    },
                )]
                .into_iter()
                .collect(),
            },
            vec!["paperless".to_string()],
            &Default::default(),
        )
        .map(|_| SetAllOutcome::default());
        assert_eq!(set_all_exit_code(refused), 2);
    }

    fn inventory(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(name, ip)| ((*name).to_string(), (*ip).to_string()))
            .collect()
    }

    #[test]
    fn target_ip_from_inventory_resolves_an_explicit_host() {
        let hosts = inventory(&[("auberge", "203.0.113.10"), ("hermes", "198.51.100.7")]);
        assert_eq!(
            target_ip_from_inventory(Some("hermes".to_string()), hosts).unwrap(),
            "198.51.100.7"
        );
    }

    #[test]
    fn target_ip_from_inventory_rejects_an_unknown_host_naming_the_inventory() {
        let hosts = inventory(&[
            ("vibecoder", "192.0.2.30"),
            ("auberge", "203.0.113.10"),
            ("hermes", "198.51.100.7"),
        ]);
        let err = target_ip_from_inventory(Some("ghost".to_string()), hosts).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Host 'ghost' not found in inventory. Available: auberge, hermes, vibecoder"
        );
    }

    // `cargo test` runs without a TTY, so this is the scripted path: no
    // picker can be drawn, and the error must name both flags because either
    // one resolves the input (--ip being the not-in-inventory escape hatch).
    #[test]
    fn target_ip_from_inventory_names_both_flags_when_it_cannot_prompt() {
        let hosts = inventory(&[("auberge", "203.0.113.10"), ("hermes", "198.51.100.7")]);
        let err = target_ip_from_inventory(None, hosts).unwrap_err();
        assert_eq!(
            err.to_string(),
            "2 hosts to choose from and stdin is not a terminal — pass -H <host> (or -i <ip>)"
        );
    }

    // One candidate is not a choice (select_item semantics). Safe here: the
    // plan preview and the `Proceed?` gate still stand between this and any
    // Cloudflare write, and confirm() refuses off-terminal without --yes.
    #[test]
    fn target_ip_from_inventory_auto_selects_a_lone_host() {
        let hosts = inventory(&[("auberge", "203.0.113.10")]);
        assert_eq!(
            target_ip_from_inventory(None, hosts).unwrap(),
            "203.0.113.10"
        );
    }

    #[test]
    fn target_ip_from_inventory_says_how_to_populate_an_empty_inventory() {
        let err = target_ip_from_inventory(None, inventory(&[])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "No hosts configured — run `auberge host add`"
        );
    }

    #[test]
    fn resolve_target_ip_prefers_an_explicit_ip() {
        assert_eq!(
            resolve_target_ip(None, Some("203.0.113.10".to_string()), false).unwrap(),
            "203.0.113.10"
        );
    }

    // --strict documents itself as non-interactive, so it bails before the
    // Inventory is even read: no picker, no lone-host auto-select.
    #[test]
    fn resolve_target_ip_strict_never_prompts() {
        let err = resolve_target_ip(None, None, true).unwrap_err();
        assert!(err.to_string().contains("strict mode"));
    }
}
