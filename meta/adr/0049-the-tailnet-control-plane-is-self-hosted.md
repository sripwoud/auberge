# ADR-0049: The tailnet control plane is self-hosted, not Tailscale SaaS

## Status

Accepted, 2026-08-29. Decided in #510; executed by the #712 flag day.

## Decision

**The tailnet's coordination server is the headscale instance on the auberge Host** (`hs.{{ domain }}`), one instance fleet-wide. Devices enroll against it with `tailscale up --login-server`; the Tailscale SaaS tailnet is left entirely — its last member (`vieille-auberge`) retires with that box, then the SaaS tailnet is deleted.

What the repo owns as a consequence:

- **Control-plane availability.** If the Host is down, no enrollments and no new handshakes; established WireGuard paths keep flowing, and peer relay resilience is bought back from Tailscale's public DERP map as a fallback region (#709) — relay traffic stays end-to-end encrypted either way.
- **Control-plane state.** `/var/lib/headscale` (SQLite + noise keys) is already a Backup Recipe (`headscale.meta.yml`); losing it means re-enrolling every device against fresh keys.
- **The one mandatory public hostname.** `hs.{{ domain }}` must exist in public DNS, unproxied: its client is by definition not on the tailnet yet, and at every boot `tailscaled` resolves the control URL before the tailnet's own DNS exists. Every other App hostname resolves through Blocky over the tailnet (ADR-0003) and never needs a public record.
- **Identity.** Users are admin-created containers, not SSO logins: one user, servers stamped `tag:server` via their pre-auth key. Enrollment goes through `auberge headscale add-user` / `add-key` (#707, #711).
- **DNS pushed from the repo, not a console.** The SaaS admin console pushed Blocky as the tailnet's global resolver; that setting moves into the headscale role template (#708), versioned and fenced like everything else.

## Why

#510: two headscale instances ran for four months with **zero enrolled nodes** while every device sat on Tailscale SaaS. The deploy was carried by a gate that could never fail (#710), and the adoption decision existed nowhere — `CONTEXT.md` and `docs/applications/networking/headscale.md` described a headscale world while reality ran on SaaS. The choice had to be made explicit in one direction or the other.

Adopt won on dependency removal. The SaaS control plane sees device metadata (names, node keys, IPs, endpoints) and holds the tailnet's DNS configuration in a console outside the repo — the last piece of the serving path not self-hosted, in a fleet that already runs its own DNS resolver, DoT endpoint, ingress, and relay host. The build cost was already paid: role, CLI surface, Caddy vhost, Backup Recipe, docs all existed before the decision did.

### What it costs

- The SaaS free tier covers this fleet with room to spare, with OAuth enrollment and a control plane on someone else's pager. Both given up knowingly; the win is sovereignty, not features.
- Enrollment UX is our CLI, and its first real use found it broken against headscale 0.29.3 (#707): the wrapper scripts a remote binary whose CLI contract Renovate bumps with nothing exercising it. Self-hosting the control plane means owning that class of drift.

## Alternatives considered

- **Decommission both instances and stay on SaaS** — #510's other branch, and the cheaper one operationally. Rejected: it re-commits the fleet's device metadata and DNS control to a third party permanently, for the price of keeping one already-built service running.
- **Keep both instances.** Rejected without much argument: two servers claiming the same `server_url` can serve at most confusion — an enrollment routed to the instance whose noise keys differ fails opaquely. The vieille instance is disabled as a #712 prerequisite.
