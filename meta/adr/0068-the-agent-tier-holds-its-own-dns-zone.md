# ADR-0068: The agent tier holds its own DNS zone

## Status

Accepted, 2026-09-01. Decided in #756, blocks #740.

## Decision

**The agent tier gets its own registered domain (`agents_domain`), onboarded as its own ordinary Cloudflare zone — not a subdomain of the parent domain.** A separate API token, `agents_cloudflare_dns_api_token`, scoped to only that zone (`Zone:DNS:Edit` + `Zone:Zone:Read`), answers DNS-01 ACME challenges for the agent tier — starting with `essaim.{agents_domain}` (#740). The fleet's existing `cloudflare_dns_api_token` stays scoped to the parent domain's zone and never lands on `ruche`.

## Why

ADR-0054 assumes `ruche`, the agent Host, is compromisable. Serving a real cert for the aoe dashboard needs ACME; `ruche` has no public ingress, so DNS-01 is the only challenge it can answer. A Cloudflare API token is zone-scoped, not record-scoped: the smallest token able to complete a DNS-01 challenge for the parent domain can also rewrite MX and every other record in that zone, including the paths used for account recovery. A wholly separate domain, holding only agent-tier records, bounds a leaked token's blast radius to that domain — worst case is losing `agents_domain`, not the parent domain's mail or any other Host's records.

The zone-per-subdomain design considered first (`agents.{domain}`, NS-delegated from the parent zone) turned out not to be available: Cloudflare's dashboard rejects onboarding a subdomain as its own zone outright ("provide the root domain, not a subdomain"), and their docs confirm managing a subdomain as an independent zone ("Subdomain Setup") is Enterprise-only. A second root domain sidesteps that restriction entirely and reuses the exact mechanism already in place for the parent domain (`cloudflare_dns_api_token`, the `dns_record` role, Caddy's `dns cloudflare` directive) — no new DNS provider, no new Caddy plugin, no new ansible role.

## Trade-off

- A second domain to register and renew, instead of a subdomain of one already owned.
- `essaim.{agents_domain}` is the aoe dashboard's origin (#740) and, once installed as a PWA, a device binds to that exact origin — the domain must be settled before the first install, or every device re-installs on a change.

## Alternatives considered

- **`agents.{domain}` as an NS-delegated subzone of the parent Cloudflare zone** — rejected: not available below Cloudflare's Enterprise tier; the dashboard's own onboarding flow refuses a subdomain as a zone name.
- **Reuse `cloudflare_dns_api_token`, scoped to the parent zone** — rejected: Cloudflare has no record-path scoping, so any token able to complete DNS-01 for the parent domain already carries edit rights over the whole zone. `ruche`'s compromise radius would include the parent domain's MX and every other Host's DNS records.
- **Delegate `agents.{domain}` to a second DNS provider** (e.g. deSEC) that supports finer-grained tokens — rejected for now: real new surface (a new ansible DNS provider integration, a new Caddy DNS plugin build, a second DNS vendor dependency) to solve a problem a second domain solves with zero new code.
- **HTTP-01 challenge** — rejected: `ruche` is tailnet-only by ADR-0054, with no port 80 reachable from Let's Encrypt.
