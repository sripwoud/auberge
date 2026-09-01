# ADR-0060: The tailnet DNS check queries the resolver's Host, not the App's

## Status

Accepted, 2026-08-31. Decided in #760; implements the fifth decision bullet of [ADR-0003](./0003-tailnet-only-app-dns.md), which the code never did. Pays the cost [ADR-0059](./0059-a-tailnet-only-apps-address-is-per-app.md) deferred.

## Decision

**A Tailnet-only App's check is two addresses.** `app_verify_config` asks the tailnet's resolver whether `{app}_subdomain.{domain}` answers with `{app}_tailscale_ip`. It used to read that one key into both `resolver_ip` and `expected_ip` — the resolver to ask, and the answer that is correct — which are the same string only while the App runs on the resolver's own Host.

**The resolver is the Host the Blocky gate selects, at the address `hosts.toml` cached for it.** `hosts::serving_hosts` is the [ADR-0051](./0051-the-headscale-gate-is-real-and-config-owned.md) gate shape — config alone answers it, a blank value is no answer — read through [ADR-0058](./0058-config-answers-per-host.md)'s host-scoped view, and `headscale`'s existing "which Host serves?" lookup now reads the same function with its own key. The address is the `tailscale_ip` that `auberge host detect-tailscale-ip` writes, already the roster's answer to "where is this Host on the tailnet" for `dns set-all`.

**Not knowing where the resolver is fails the App's check; it never skips it.** Four ways not to know — no Host answers the gate, several do, the serving Host has no cached address, the cached address is not in `100.64.0.0/10` — each become a named reason on the deploy's error list rather than a `None` that reads as "nothing to verify". A Tailnet-only App's records exist in that resolver or nowhere, so a check that cannot be run has not passed.

**Only a Tailnet-only App pays.** The public branch never consults the resolver, so a fleet that runs no Blocky at all deploys exactly as it did.

## Why

ADR-0003's fifth bullet says the check queries Blocky. The implementation queried `{app}_tailscale_ip`, and on a fleet of one Host that _was_ Blocky's address, so nothing ever disagreed. This is a regression against a written decision, not a gap in one.

It was dormant, and ADR-0059 is what wakes it: no App set `{app}_tailscale_ip`, so the tailnet branch never ran; ADR-0059 makes setting that key the documented way to publish an off-Host Tailnet-only App. The first one — `essaim` on ruche (#740) — would have got correct publication and a red deploy, because the check would have asked the App's own Host, which serves the App and no resolver, and read the timeout as a DNS fault.

The two tests say it better than prose: reverting the fix leaves `test_app_verify_config_tailnet_on_the_resolvers_own_host` green and turns `..._queries_the_resolver_for_the_apps_address` red. The first is the whole world this code was written in.

### What it costs

**A fleet where several Hosts answer `blocky_subdomain` cannot deploy a Tailnet-only App until it withdraws the gate.** That is this fleet today: the key is answered top-level, so both `auberge` and `vieille-auberge` claim it. ADR-0058 already named that state a bug — two Hosts answering means two resolvers racing the `dns.{domain}` certificate — and already prescribed the remedy, `[hosts.<name>] blocky_subdomain = ""` on the Hosts that do not serve. Surfacing it at the first deploy that depends on the answer is the point; the error names the Hosts and the remedy.

**`hosts.toml`'s `tailscale_ip` becomes load-bearing for a check that used to need no roster at all.** It is a cache the operator fills by running one command, and an unfilled one now fails a Tailnet-only App's deploy. Accepted: the alternative is a check pointed somewhere unverified, and the error says which command to run.

## Alternatives considered

- **Skip the check when the resolver is unlocatable**, as the code does today for an App with no address. Rejected: silent skipping is exactly what kept this dormant for three months of ADRs. The check's whole job is catching a publication that did not happen, which is indistinguishable from a check that did not run.
- **Count only gate-answering Hosts that have a cached `tailscale_ip`.** It would make this fleet work today without withdrawing anything. Rejected: `tailscale_ip` is a local cache, so "never detected" and "not on the tailnet" are the same absence, and the filter would silently pick a resolver out of a two-resolver misconfiguration instead of reporting it.
- **Ask the serving Host for its address over SSH at check time.** Rejected: a round-trip per deploy for a value that changes only on re-enrolment, when the roster already caches it for `dns set-all`.
- **Read the expected address out of Blocky's rendered `customDNS`.** Rejected: the check exists to test publication from outside. Sourcing its expectation from the artifact it verifies makes it agree with itself by construction.
- **Resolve `{blocky_subdomain}.{domain}` to find the resolver.** Rejected: circular — that name is published by the resolver being located, over the tailnet, by the record this would need answered first.
