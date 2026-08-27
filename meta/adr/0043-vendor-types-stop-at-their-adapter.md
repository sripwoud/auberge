# ADR-0043: A vendor client is reached through a crate-local trait

## Status

Accepted, 2026-08-27. **Generalises the `DnsLookup` seam** that `services/dns_verify` has had since the DNS Publication check was written, and applies it to the record side it was always the odd sibling of.

## Decision

A third-party SDK the crate calls out to is reached through a crate-local trait. The vendor's own types are translated at exactly one module — the adapter — and nothing above it names them. Three rules follow:

- **The handshake lives in the constructor.** Credentials, zone lookup, connection setup: whatever the vendor needs to become usable happens in the adapter's constructor, so the trait's methods are plain reads and writes and a test double is a struct literal. `CloudflareDns::connect()` resolves the Domain to its zone; `DnsRecords` has `list_records`, `set_a_record`, `delete_a_record`, and the Domain it is bound to.
- **Logic is a free function over the trait, not a method on the client.** `plan_set_all`, `apply_set_all`, `migrate_all`, and `status` take `&D: DnsRecords`, the shape `verify_a_record<L: DnsLookup>` already had. A method on the concrete client is reachable only by constructing the client, which is the thing that needs a network.
- **Plan before apply.** Work that decides what to write is separated from the work that writes it, and the applying half returns its outcome rather than exiting. A failed write is recorded in the outcome, never propagated, so both `--output` modes still get their ADR-0004 body on the failure path and the command maps the outcome onto the Backup Verdict exit codes (0 all written, 1 at least one write failed, 2 operational error) that `backup verify` and `versions` already use.

`tests/vendor_types_stay_in_adapter.rs` fences the boundary in both directions: no module outside the declared adapter may name `cloudflare::` or `hickory_resolver::`, **and** each declared adapter must still name its vendor — otherwise renaming or emptying an adapter would leave the scan passing over a domain with nothing in it.

Confined vendors are SDKs whose types would otherwise reach a caller as domain values. A transport crate is not one: `reqwest` is used directly by `commands/versions` and `services/bichon/api`, where what crosses the boundary is JSON the module has already parsed into its own types, and there is no vendor vocabulary for a caller to inherit.

## Why

`DnsService`'s only constructor performed a zone lookup, so all five of its async methods were unreachable from a test and all five were untested. The consequences compounded outward. `dns set-all` was 232 lines and ten parameters, of which the ADR-0003 partition — tailnet-only Apps get no Cloudflare A record — was the sole slice anyone had managed to extract and test; the rest, including the effective-IP override and the ordering the output depends on, was verified by running it against the real zone. It ended in `process::exit(1)`, so even the failure path could not be observed: a run that failed to create records emitted no JSON body at all, only a bare non-zero exit.

The vendor types leaked in step with that. Reading an IP out of a record meant matching `DnsContent::A { content }`, which `commands/dns` did in three places — once to render the `TYPE`/`CONTENT` columns, twice to filter A records out of a status report — so the command module imported a Cloudflare enum to answer a question about the user's own Domain. The A-record filter that repeats through the service (`matches!(r.content, DnsContent::A { .. })`) is one `a_ip()` on a crate-local record; the seven-arm render match is `kind()` and `value()` next to the enum they exhaust.

The dead `_production_override` parameter is the same failure seen from the other end: every subcommand threaded a `--production` flag into a constructor that ignored it and hardcoded `Environment::Production`. Nothing exercised the constructor, so nothing noticed. The parameter is gone; the flag survives on `dns delete`, where it does mean something — it escalates the confirmation to a typed subdomain name and is reported in the JSON — and `main` now destructures it as `production: _` on the four subcommands where it does not, which states at the dispatch site that nothing downstream reads it.

Translating rather than re-exporting is what buys the isolation. `cloudflare` 0.14's `DnsContent` is a closed enum, so a new record type upstream breaks one match in one file instead of arriving at a caller as a variant it does not handle — and swapping providers becomes one module rather than a search across the command layer. The cost is a translation function and a parallel enum, paid once, against 24 tests that previously could not be written.
