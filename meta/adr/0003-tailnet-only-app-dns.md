# ADR-0003: Tailnet-only Apps publish DNS via Blocky only

## Status

Accepted, 2026-05-05.

## Context

Auberge deploys two kinds of apps to a single host: public apps reachable from the open internet (e.g. `freshrss`, `navidrome`) and apps intended only for the user's tailnet (e.g. `bichon`, `paperless`, `cockpit`). Both kinds share the same Caddy reverse proxy, the same TLS-via-DNS-01 cert flow, and historically the same DNS publication path: a Cloudflare A record created by the `dns_record` role.

Three problems accumulated:

1. **Inconsistent tailnet-only patterns.** `cockpit` published `A → Tailscale CGNAT IP`. `paperless` published `A → host public IP` plus a `blocky_tailscale_bound_domains` override. `bichon` published nothing. Each "tailnet-only" promise meant something different.

2. **Headscale didn't push Blocky.** Despite the user's intent that tailnet membership implies Blocky filtering, headscale was configured to push `1.1.1.1` as the resolver. The `blocky_tailscale_bound_domains` mechanism only worked for clients with manual DoT configured — i.e. the user's personal devices, not new tailnet members.

3. **Public DNS records for tailnet-only apps leaked existence.** Even when an A record correctly pointed at a Tailscale CGNAT IP, anyone in the world could `dig` the hostname and learn the service existed and was Tailscale-routed.

## Decision

- Headscale pushes Blocky as the resolver for `*.{{ domain }}` via split-DNS (`nameservers.split`); all other queries continue to use `1.1.1.1`.
- Tailnet-only Apps publish DNS exclusively via Blocky's `customDNS` map. They do not create Cloudflare A records.
- Public Apps continue to publish a Cloudflare A record via the `dns_record` role.
- The Blocky `customDNS` list is derived at run-time inside the Blocky role from `tailnet_only: true` declarations in playbook meta files. There is no parallel hand-maintained list.
- `auberge deploy` performs a runtime DNS-resolution check at the end of each app's deploy (Blocky for tailnet-only; `1.1.1.1` for public) and fails the deploy if the record doesn't resolve correctly.

## Consequences

**Positive:**

- "Tailnet-only" becomes literally true: `dig` from a non-Blocky resolver returns NXDOMAIN. No public-DNS leak.
- Single source of truth per app type — Cloudflare for public, Blocky for tailnet-only — selected by one flag.
- New tailnet members get Blocky DNS automatically via headscale push; no per-device DoT configuration needed.
- Bichon-class bugs (role missing DNS publication) become visible at deploy time via the runtime check.

**Negative:**

- Tailnet members who deliberately set `tailscale set --accept-dns=false` no longer auto-resolve internal apps; they must manually point at Blocky. Acceptable: it's a deliberate opt-out from magic-DNS.
- Blocky becomes load-bearing for `*.{{ domain }}` resolution on the tailnet. Mitigated by split-DNS keeping general internet DNS on `1.1.1.1` — Blocky outage doesn't break browsing, only internal apps.
- Existing Cloudflare A records for `cockpit` and `paperless` become stale and must be deleted as a one-time cutover.

## Alternatives considered

- **Keep `1.1.1.1` as tailnet resolver.** Forces Cloudflare A → Tailscale IP for every tailnet-only app; preserves status-quo brittleness (manual DoT on each device, paperless-style two-truth split). Rejected: doesn't deliver "on tailnet → automatic Blocky."
- **Headscale-side global Blocky push (no split-DNS).** Strongest filtering coverage but makes Blocky a single point of failure for _all_ DNS resolution including external sites. Rejected: blast radius of a Blocky outage too large.
- **Cloudflare A → Tailscale IP for tailnet-only apps (pattern A) with split-DNS.** Reachable, no two-truth problem after `blocky_tailscale_bound_domains` retirement. Rejected: still publishes the existence of tailnet-only services in public DNS, which contradicts what "tailnet-only" claims.
- **CLI-derived Blocky list.** `auberge deploy` injects the list via `--extra-vars`. Rejected: breaks direct `ansible-playbook` invocations without surprise; auberge's CONTEXT positions the CLI as canonical but Ansible as not hidden.
- **Compile-time test for DNS publication invariants.** A Rust test that parses every role's tasks. Rejected: parser fragility outweighs the benefit in a single-operator homelab where deploys happen frequently and the runtime check fires within minutes of any regression.

## Implementation notes

### `subdomain` lives in the Playbook Meta, with operator override

The Blocky role composes each tailnet-only FQDN as `<effective_subdomain>.<domain>`, where `effective_subdomain = lookup('vars', '<app>_subdomain', default=meta.subdomain)`. The meta's `subdomain` field is the canonical _default_; the operator's `<app>_subdomain` in `config.toml` (which the CLI propagates to Ansible as an extra-var) is the _override_.

Why both layers:

- Caddy and the TLS-via-DNS-01 cert flow already consume the operator override via the role-default → extra-var precedence (extra-vars beat defaults). For Blocky's FQDN to match Caddy's vhost and the cert's SAN, Blocky must consult the same override.
- An earlier draft used `lookup('vars', ...)` with `default=app` as the _only_ source. That worked only because every role default happened to equal the app name and would have silently broken if a default ever diverged. Promoting `subdomain` to the meta gave a non-fragile default; layering the override on top restores the operator-renaming UX without reintroducing the original fragility — the meta is consulted unconditionally as the fallback, so a missing extra-var is no longer a silent failure.

App identity for the override lookup is derived from the meta filename (`<app>.meta.yml` → `<app>`), matching the Rust-side convention in `playbook_meta.rs::load_for_app`. Public Apps may still rely on role defaults; only tailnet-only Apps need the meta declaration.

### Split-DNS target IP auto-detection

The split-DNS target IP is exposed as `headscale_split_dns_target_ip`. The Headscale role auto-detects it from the host's Tailscale interface (`tailscale status --json`), matching the pattern Blocky uses for its own bind address. Operators may override it via inventory or extra-vars when Blocky and Headscale run on different hosts.

### First-deploy ordering caveat

In `infrastructure.yml`, the `headscale` role runs before the `tailscale` role (Headscale must be reachable before the host can `tailscale up` against it as login server). On the very first deploy, `tailscale status` returns no IP, the `set_fact` is skipped, and the rendered config omits the `split` block. A second deploy run — after `tailscale up` succeeds — populates the IP and the rendered config gains the `split` block, triggering the `Restart headscale` handler. This two-pass behaviour is consistent with how the Blocky role already handles its own tailnet bind address.
