//! ADR-0043: a vendor SDK is reached through a crate-local trait, and the
//! vendor's own types are translated at exactly one module.
//!
//! Both halves are fenced. A vendor named outside its adapter is the drift this
//! catches; an adapter that stops naming its vendor is how the scan would
//! silently empty out, so that is an assertion too. The third way it could
//! empty out — a walk that stops reaching the crate's modules — is pinned by
//! `crate_source::modules` itself, and inherited here (#679).

mod crate_source;

use crate_source::modules;

/// `(vendor crate, the one module allowed to name it)`.
///
/// `cloudflare` is `dns`'s record side, `hickory_resolver` its query side —
/// the two seams (`DnsRecords`, `DnsLookup`) the rule was written from.
const CONFINED_VENDORS: &[(&str, &str)] = &[
    ("cloudflare", "src/services/cloudflare_dns.rs"),
    ("hickory_resolver", "src/services/dns_verify.rs"),
];

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

#[test]
fn vendor_types_are_named_only_by_their_adapter() {
    let mut offenders: Vec<String> = Vec::new();
    for module in modules() {
        let relative = &module.repo_relative;
        for (vendor, adapter) in CONFINED_VENDORS {
            if relative != adapter && names_vendor(&module.source, vendor) {
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
///
/// The adapter is looked up in the walk rather than read from disk beside it,
/// so an adapter the walk cannot reach fails here too — the same file read two
/// ways is how a fence comes to assert against a file its scan never saw.
#[test]
fn every_adapter_still_names_its_vendor() {
    let walked = modules();
    for (vendor, adapter) in CONFINED_VENDORS {
        let module = walked
            .iter()
            .find(|module| module.repo_relative == *adapter)
            .unwrap_or_else(|| panic!("{adapter} must exist to confine `{vendor}`"));
        assert!(
            names_vendor(&module.source, vendor),
            "{adapter} no longer names `{vendor}::` — either the adapter moved \
             (update CONFINED_VENDORS) or the vendor is gone (drop the row)"
        );
    }
}
