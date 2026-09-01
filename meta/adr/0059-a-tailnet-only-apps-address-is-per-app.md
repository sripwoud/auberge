# ADR-0059: DNS Publication resolves a Tailnet-only App's address per App

## Status

Accepted, 2026-08-31. Decided in #755; surfaced preparing #740's `essaim.agents.{domain}`. The deploy-time check it left pointed at the wrong Host is fixed by [ADR-0060](./0060-the-tailnet-dns-check-queries-the-resolver-host.md). Amends the fourth decision bullet of [ADR-0003](./0003-tailnet-only-app-dns.md), which named a `customDNS` _list_ and left the address it maps to implicit; [ADR-0052](./0052-the-tailnets-global-resolver-is-the-hosts-blocky.md) inherited that assumption and otherwise stands.

## Decision

**Blocky's `customDNS` map carries one address per entry.** The `blocky` role builds `blocky_tailscale_domain_addresses` — FQDN to address — from the fleet-wide `*.meta.yml` scan it already ran, and `config.yaml.j2` emits each entry's own address. It used to build `blocky_tailscale_bound_domains`, a flat list of FQDNs, and interpolate one `blocky_tailscale_ipv4` for all of them.

**`<app>_tailscale_ip` answers the same question on both publication channels.** `discover_all_subdomains` reads it into `SubdomainEntry.ip_override` for a Tailnet-only App exactly as it always did for a Public App; the Tailnet-only branch constructed `None` unconditionally. The two channels now agree on what an App's tailnet address is, which is the precondition for either of them being able to say where an App is.

**An App declaring no address of its own resolves to the Blocky Host's.** That is every App in the fleet today — no `<app>_tailscale_ip` is set anywhere — so the rendered map does not move. The fallback is not a convenience: for an App that does run on the Blocky Host it is the only answer that stays correct across a re-enrolment, since the address is discovered on each deploy rather than written down.

**The address is declared fleet-wide, never per Host.** The map is built `run_once`, on the controller, in the variable scope of the Host the infrastructure play targets. A `[hosts.<name>]` override ([ADR-0058](./0058-config-answers-per-host.md)) merges into _that Host's_ extra-vars, so an address scoped to the Host an App runs on is precisely the one Blocky cannot see. `<app>_tailscale_ip` stays a top-level key: it is a fact about where an App is, published by a resolver somewhere else.

**A Host with no tailnet address of its own publishes no map at all**, including for Apps that declare one. Unchanged, and worth stating now that entries can carry their own addresses: a resolver that is not listening on the tailnet cannot be asked, so an entry it could technically render is not an entry anyone reaches.

## Why

ADR-0003 moved Tailnet-only Apps off Cloudflare and onto Blocky's `customDNS`, and ADR-0052 made that Blocky the tailnet's only resolver. Both were written for a fleet of one Host, where "the App's address" and "the resolver's address" are the same string, and neither had a reason to say which one it meant. The role picked the resolver's, because that is the one it had.

The discovery was never the narrow half. Blocky's meta scan is `run_once` + `delegate_to: localhost` over every `*.meta.yml` in the playbooks directory, regardless of which playbook deploys the App — so a Tailnet-only App on a second Host is already found, and then published at the wrong address. That shape is why this is a mapping change and not a discovery change, and why narrowing the scan to "the Apps this play deploys" would delete entries rather than fix them. The fence over it asserts the scan's breadth for that reason.

The failure it produces is the quiet kind. `dig` answers, Blocky is up, the deploy reports success, and the name resolves — to a Host that does not serve the App. Nothing in the publication path compares the address it publishes against the Host the App runs on, because until there was a second Host there was nothing to compare.

### What it costs

**A second Host's Tailnet-only App needs a config key that the App's own role would otherwise derive.** `actual`, `bichon`, `cockpit` and `paperless` each discover `<app>_tailscale_ip` from `tailscale status` on their own Host when it is unset. That fact is host-scoped and is set during the App's play; the Blocky play, running against a different Host, cannot read it. So the operator writes the address down once per off-Host App.

Accepted, because the alternative is a cross-host fact exchange for a value that changes only on re-enrolment, and because the value is written down once rather than tracked.

**Setting the key wakes a deploy-time DNS check that assumes the same thing this ADR unassumes.** `app_verify_config` reads `<app>_tailscale_ip` as _both_ the resolver to query and the address to expect, which is only ever one string on a single-Host fleet — and is not what ADR-0003's fifth decision bullet specified, which says the check queries _Blocky_ for a Tailnet-only App. It has been dormant — no App sets the key today, so the check returns `None` and never runs — and setting the key for an off-Host App is what wakes it, pointed at a Host that serves the App and no resolver. So the first off-Host Tailnet-only App gets correct publication and an aborted deploy. Carried as #760 rather than fixed here: separating the two addresses needs the CLI to answer "where is Blocky?", which is a Host lookup that does not exist yet. **Fixed by [ADR-0060](./0060-the-tailnet-dns-check-queries-the-resolver-host.md)**, which builds that lookup: the check now queries the Host the Blocky gate selects, and no longer skips silently when it cannot find one.

## Alternatives considered

- **Keep one address and route a second Host's Apps through the Blocky Host's Caddy.** Rejected: it makes the DNS Host a mandatory hop for every Tailnet-only App in the fleet, to avoid writing down an address. It also inverts what "tailnet-only" buys — the tailnet already routes to every Host directly.
- **Have each App's play write its address into a fact Blocky reads.** Rejected: Ansible facts are host-scoped, so this is a cross-host exchange (`hostvars`, a cache, or an ordering constraint between two plays) built to carry a value the operator already knows. It also makes the map's contents depend on which Apps a given run touched, where today the scan is fleet-wide by construction.
- **Derive the address from the Host an App is deployed to, via the inventory.** Rejected for now, not on principle: the Meta says nothing about which Host runs an App, so the derivation needs a Host-to-App mapping that does not exist yet. When one does, this key becomes its fallback rather than its replacement.
- **Make `<app>_tailscale_ip` a per-Host key under `[hosts.<name>]` ([ADR-0058](./0058-config-answers-per-host.md)).** Rejected: it reads as the natural home and is the one place the value provably cannot be read from. See the fourth decision above.
- **Publish Tailnet-only Apps through headscale's split DNS instead of Blocky.** Rejected: ADR-0052 already made Blocky the tailnet's global resolver, so a split route for `{{ domain }}` would point at Blocky anyway. This would add a second publication path to keep in agreement with the first.
- **Keep `blocky_tailscale_bound_domains` and add a parallel override map beside it.** Rejected: two facts that must be keyed identically, one of which is silently ignored when they are not. The list was already the map with its values thrown away.
