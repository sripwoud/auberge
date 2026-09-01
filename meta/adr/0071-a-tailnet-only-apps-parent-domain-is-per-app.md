# ADR-0071: DNS Publication resolves a Tailnet-only App's parent domain per App

## Status

Accepted, 2026-09-01. Decided in #740, preparing `essaim.{agents_domain}`. Completes [ADR-0059](./0059-a-tailnet-only-apps-address-is-per-app.md), which made the _address_ per App and left the _domain_ implicit; the mechanism it needs is [ADR-0068](./0068-the-agent-tier-holds-its-own-dns-zone.md)'s second zone.

## Decision

**A Playbook Meta may name the Key Registry key holding its App's parent domain, as `domain_key:`.** DNS Publication composes the App's FQDN as `<effective subdomain>.<that key's answer>`; a Meta naming none composes against `domain`, which is every App in the fleet but one. `aoe.meta.yml` names `agents_domain`.

**It names a key, not a domain.** So a Host serving a second zone answers for itself under `[hosts.<name>]` ([ADR-0058](./0058-config-answers-per-host.md)), and no domain of the operator's is written into the repo — the same reason `subdomain:` is a label and `<app>_subdomain` is the answer.

**Both publication consumers read the field.** Blocky's `customDNS` map reads it off the Meta it already slurps; the deploy-time resolution check reads it through `services::dns::app_parent_domain`, which replaced the one `domain` `run_dns_checks_for_run` resolved per run. A declaration one consumer honoured and the other ignored would publish a name under one domain and verify it under another, and report success for both.

**An App whose key is unanswered publishes nothing.** Not a fallback to `domain`: an operator who never onboarded the second zone has no name there, and composing one against the fleet's domain would publish `essaim.{domain}` — a name in the wrong zone, with a real address behind it. Falling back the other way is worse still: `essaim.` is what an empty domain yields, and blocky refuses to load a config holding it, which takes the resolution of every other Tailnet-only App down with it.

**The field's reach is the Tailnet-only channel.** Cloudflare publication still composes one zone per run (`plan_set_all` takes a single `domain`, and the adapter's zone is resolved in `CloudflareDns::connect`), so a Public App declaring `domain_key` would write an A record into the wrong zone rather than fail. `tests/tailnet_only_parent_domain.rs` refuses that combination until the Cloudflare half is plumbed.

## Why

ADR-0059's own framing is the argument: "the two channels now agree on what an App's tailnet address is, which is the precondition for either of them being able to say where an App is." The domain half of the same composition stayed a constant, because until ADR-0068 the fleet had one zone and "the App's domain" and "the fleet's domain" were the same string — the identical shape, one layer up, that ADR-0059 removed from the address.

The failure it produces is the same quiet kind. `essaim.{domain}` resolves, on the parent domain's public zone, to a `100.64.0.0/10` address — so `dig` answers, the deploy reports success, and Caddy on `ruche` holds a certificate for `essaim.{agents_domain}`, a name nothing publishes. The dashboard is unreachable and every layer says it is fine.

The fence evaluates the role's own `vars:` chain rather than scanning it, with `lookup('vars', …)` shimmed, because the mutation that matters — reading the key and then composing against `domain` anyway — leaves the field's name in the file for a text scan to find. That is the mutation that defeated #755's first fence, named in `tests/blocky_tailnet_addresses.rs`.

## What it costs

**A second key to answer before the agent tier resolves.** `agents_domain` and `aoe_tailscale_ip` are both fleet-wide keys the operator writes down once, and neither has a default the repo could supply. The unanswered case is silent by design (the App drops out of the map), so the operator's evidence that it worked is `dig essaim.{agents_domain}` against the tailnet resolver — which is what the deploy-time check automates once the App is on a composition that runs it (#743).

**A Meta field two languages read.** Held together by `tests/tailnet_only_parent_domain.rs`: the crate's deserialization and the role's expression are asserted against one spelling and one default.

## Alternatives considered

- **Put the App's whole FQDN in the Meta.** Rejected: `subdomain:` is already the label half and `<app>_subdomain` its override, so this would be a third spelling of a name that has two, and it writes the operator's domain into the repo.
- **A `<app>_domain` config key per App, read the way `<app>_tailscale_ip` is.** Rejected: it makes every App's parent domain a thing the operator can set and must not, where `domain_key` is a repo-owned statement about which zone an App belongs to. It would also have no default — 20 Apps would each need the fleet's domain written into their own key.
- **Host-scope `domain` itself for the agent Host** (`[hosts.ruche] domain = agents-example.com`). Rejected for the reason ADR-0059 gives for the address: the map is built `run_once` in the variable scope of the Host the infrastructure play targets, so a value scoped to the Host an App runs on is precisely the one Blocky cannot see. It would also re-point every other key that composes against `domain` on that Host.
- **Derive the domain from the Host an App is deployed to.** Rejected for now, exactly as ADR-0059 rejected it for the address: the Meta says nothing about which Host runs an App, so the derivation needs a Host-to-App mapping that does not exist yet.
- **Leave the composition alone and publish the agent tier through a Cloudflare A record in its own zone.** Rejected: it is [ADR-0003](./0003-tailnet-only-app-dns.md)'s "pattern A", which publishes the existence of a tailnet-only service in public DNS — and the agent tier is the one Host whose reachability is most worth not advertising (ADR-0054).
