//! ADR-0043: a vendor SDK is reached through a crate-local trait, and the
//! vendor's own types are translated at exactly one module.
//!
//! Both halves are fenced. A vendor named outside its adapter is the drift this
//! catches; an adapter that stops naming its vendor is how the scan would
//! silently empty out, so that is an assertion too.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// `(vendor crate, the one module allowed to name it)`.
///
/// `cloudflare` is `dns`'s record side, `hickory_resolver` its query side —
/// the two seams (`DnsRecords`, `DnsLookup`) the rule was written from.
const CONFINED_VENDORS: &[(&str, &str)] = &[
    ("cloudflare", "src/services/cloudflare_dns.rs"),
    ("hickory_resolver", "src/services/dns_verify.rs"),
];

/// Every module in the crate, so the walk above is checked against a set
/// rather than trusted to have found things.
const CRATE_MODULES: &[&str] = &[
    "src/ansible_assets.rs",
    "src/commands.rs",
    "src/commands/ansible.rs",
    "src/commands/backup.rs",
    "src/commands/bichon/mod.rs",
    "src/commands/bichon/reconcile.rs",
    "src/commands/bichon/rescan.rs",
    "src/commands/bichon/verify.rs",
    "src/commands/config_cmd.rs",
    "src/commands/deploy.rs",
    "src/commands/dns.rs",
    "src/commands/headscale.rs",
    "src/commands/host.rs",
    "src/commands/select.rs",
    "src/commands/ssh.rs",
    "src/commands/sync.rs",
    "src/commands/versions.rs",
    "src/config.rs",
    "src/hosts.rs",
    "src/key_registry.rs",
    "src/main.rs",
    "src/output.rs",
    "src/playbook_meta.rs",
    "src/prompt.rs",
    "src/services.rs",
    "src/services/ansible_runner.rs",
    "src/services/backup.rs",
    "src/services/backup/executor.rs",
    "src/services/backup/recipe.rs",
    "src/services/backup/restic.rs",
    "src/services/backup/session.rs",
    "src/services/backup/verify.rs",
    "src/services/bichon/api.rs",
    "src/services/bichon/coverage.rs",
    "src/services/bichon/folder_filter.rs",
    "src/services/bichon/mod.rs",
    "src/services/bichon/rescan.rs",
    "src/services/cloudflare_dns.rs",
    "src/services/dependency_resolver.rs",
    "src/services/dns.rs",
    "src/services/dns_verify.rs",
    "src/services/inventory.rs",
    "src/services/progress.rs",
    "src/services/rsync.rs",
    "src/services/ssh.rs",
    "src/services/ssh_include.rs",
    "src/services/unit_state.rs",
    "src/signal.rs",
    "src/ssh_config.rs",
    "src/ssh_session.rs",
    "src/tool_versions.rs",
];

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found.sort();
}

/// `true` when `source` uses a path rooted at `vendor`.
///
/// The trailing `::` is what makes this a path rather than a mention: the repo
/// is full of `cloudflare_dns_api_token` config keys and prose about Cloudflare
/// dashboards, and none of those is a vendor type. A preceding identifier
/// character rules out `services::cloudflare_dns::`, which is ours.
fn names_vendor(source: &str, vendor: &str) -> bool {
    let needle = format!("{vendor}::");
    let bytes = source.as_bytes();
    let mut from = 0;
    while let Some(offset) = source[from..].find(&needle) {
        let at = from + offset;
        let after_identifier = at > 0 && {
            let prev = bytes[at - 1] as char;
            prev == '_' || prev.is_ascii_alphanumeric()
        };
        if !after_identifier {
            return true;
        }
        from = at + needle.len();
    }
    false
}

fn repo_relative(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .expect("scanned paths live under the manifest dir")
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn vendor_types_are_named_only_by_their_adapter() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);

    let mut offenders: Vec<String> = Vec::new();
    for path in &files {
        let relative = repo_relative(path);
        let source =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("{relative} must be readable: {e}"));
        for (vendor, adapter) in CONFINED_VENDORS {
            if relative != *adapter && names_vendor(&source, vendor) {
                offenders.push(format!(
                    "  {relative} names `{vendor}::` — translate it at {adapter} \
                     and give the caller a crate-local type (ADR-0043)"
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "vendor types escaped their adapter:\n{}",
        offenders.join("\n")
    );
}

/// Without this, deleting or renaming an adapter would leave the scan above
/// passing over a domain that no longer contains the thing it fences.
#[test]
fn every_adapter_still_names_its_vendor() {
    for (vendor, adapter) in CONFINED_VENDORS {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(adapter);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{adapter} must exist to confine `{vendor}`: {e}"));
        assert!(
            names_vendor(&source, vendor),
            "{adapter} no longer names `{vendor}::` — either the adapter moved \
             (update CONFINED_VENDORS) or the vendor is gone (drop the row)"
        );
    }
}

/// The scan's reach, by equality in both directions: a new module fails until
/// it is listed, and a listing the crate no longer holds fails until it is
/// removed. A floor (`>= 45`) would let modules leave the walk silently, and
/// every assertion above can pass by seeing nothing.
#[test]
fn the_scan_sees_exactly_the_modules_the_crate_holds() {
    let mut walked = Vec::new();
    rust_files(&src_dir(), &mut walked);
    let seen: BTreeSet<String> = walked.iter().map(|p| repo_relative(p)).collect();
    let listed: BTreeSet<String> = CRATE_MODULES.iter().map(|m| (*m).to_string()).collect();

    let unlisted: Vec<&String> = seen.difference(&listed).collect();
    let missing: Vec<&String> = listed.difference(&seen).collect();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "the crate's module set moved.\n  new, add to CRATE_MODULES: {unlisted:?}\n  gone, drop from CRATE_MODULES: {missing:?}"
    );
}
