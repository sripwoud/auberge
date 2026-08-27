//! ADR-0043: a vendor SDK is reached through a crate-local trait, and the
//! vendor's own types are translated at exactly one module.
//!
//! Both halves are fenced. A vendor named outside its adapter is the drift this
//! catches; an adapter that stops naming its vendor is how the scan would
//! silently empty out, so that is an assertion too.

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

fn relative(path: &Path) -> String {
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
        let relative = relative(path);
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

/// The scan is only as good as the tree it walks.
#[test]
fn the_scan_reaches_every_module_in_the_crate() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);

    assert!(
        files.len() >= 45,
        "expected the whole crate, walked only {} files",
        files.len()
    );
    for expected in [
        "src/main.rs",
        "src/services/dns.rs",
        "src/services/cloudflare_dns.rs",
        "src/commands/dns.rs",
        "src/services/bichon/api.rs",
    ] {
        assert!(
            files.iter().any(|p| relative(p) == expected),
            "{expected} was not reached by the walk"
        );
    }
}
