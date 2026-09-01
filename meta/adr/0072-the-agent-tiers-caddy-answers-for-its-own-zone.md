# ADR-0072: The agent tier's Caddy answers DNS-01 for its own zone

## Status

Accepted, 2026-09-01. Decided in #740. Implements the delivery half of [ADR-0068](./0068-the-agent-tier-holds-its-own-dns-zone.md), which named `agents_cloudflare_dns_api_token` and left "how does it reach that Host's Caddy" open.

## Decision

**The caddy role reads its ACME token through `caddy_dns_api_token`, defaulting to `{{ cloudflare_dns_api_token }}`.** The systemd drop-in that puts `CLOUDFLARE_DNS_API_TOKEN` in caddy's environment reads the indirection, never a config key. Nineteen Hosts take the default and do not move.

**`infrastructure.yml` resolves it to the agent tier's token on the Host whose config answers `aoe_subdomain`.** That key is the agent tier's serving gate in the [ADR-0051](./0051-the-headscale-gate-is-real-and-config-owned.md) shape — config alone answers it, a blank value is no answer — scoped under `[hosts.<name>]` like `headscale_subdomain` and `blocky_subdomain` beside it ([ADR-0058](./0058-config-answers-per-host.md)). Jinja evaluates only the branch it takes, so an operator with no agent tier never reads a token they never minted.

**The redirect happens in the infrastructure play, not in a play of the agent tier's own.** Caddy is a Substrate App: `infrastructure.yml` runs it against every Host on every deploy, ahead of any App play. A later play re-running caddy with a role var would leave the parent domain's token on the box between the two, which is exactly what ADR-0068 forbids — the token must never land on `ruche`, not even for one play.

## Why

Caddy is the only thing on the agent Host that needs an ACME token, and it takes one token per process. Which token is right is a property of the Host — of which zone its vhosts live in — so the answer has to come from the one mechanism that answers per Host.

The token could not simply be `cloudflare_dns_api_token` host-scoped to a different value. ADR-0068 gave the agent tier's token its own name because it is a different token with a different scope, and two names for one value is a drift that no test can see: the operator sets both, one goes stale, and the failure is a certificate that stops renewing on a Host nobody looks at. The indirection keeps one value under one name and moves only the question of which name to read.

Naming `aoe_subdomain` as the gate couples caddy's token to one App's key, which is worth saying out loud. It is the right coupling for the same reason `blocky_subdomain` gates the blocky role: the question "does this Host serve the agent tier?" has exactly one observable answer in config today, and inventing a second key to restate it would be a declaration that can disagree with the first. If a Host ever serves the agent tier's zone without serving the dashboard, that is when the gate earns a name of its own.

`tests/caddy_acme_token.rs` evaluates the playbook's expression both ways round rather than checking that a branch exists. The failure that costs something is not a missing branch but a branch resolving to the wrong token, which reads as a working deploy right up until the token leaks — and a blank gate read as an answer would hand the agent tier's token to a Host that withdrew it.

## Trade-off

- **A third key to set for the agent tier**, and one that has to be host-scoped or it re-points the whole fleet. `docs/configuration/agent-tier-dns-zone.md` says so at the table and again at the step; the failure mode if it is missed is that the parent domain's certificates stop renewing, which is loud within 60 days rather than immediately.
- **Two config keys can now answer "the ACME token"**, and which one a Host reads is decided in a playbook rather than in config. The alternative — a `caddy_dns_api_token` config key host-scoped directly — puts the decision in config and leaves ADR-0068's key read by nothing.

## Alternatives considered

- **Make `caddy_dns_api_token` a Key Registry key, host-scoped for `ruche`.** Rejected: it needs the token's value written under a second name, leaving `agents_cloudflare_dns_api_token` in the registry and read by nothing. Two names for one secret is the rotation bug above.
- **Host-scope `cloudflare_dns_api_token` itself for `ruche`** (what #740's triage comment proposed, before ADR-0068 settled on a separate domain). Rejected: it makes one key mean "the parent zone's token" fleet-wide and "the agent zone's token" on one Host, so nothing in the repo can state which zone a token is scoped to — and ADR-0068's key would be deleted to get there.
- **Gate on `group_names`, from the Host's `hosts.toml` `tags`.** Rejected: tags are ansible groups deciding which roles run, and [ADR-0062](./0062-a-hosts-trust-tier-is-a-typed-roster-field.md) is explicit that overloading them with what a Host may reach makes adding a group move a Host's trust. The same argument applies to moving its ACME token.
- **Gate on `tailnet_tag == agent`.** Rejected: the Tailnet Tag reaches no Playbook by design, and it answers a network-trust question rather than a "which zone does this Host serve" one. A `standby` agent Host would answer differently on the two.
- **Let `ruche`'s own play (#743) re-run the caddy role with the token as a role var.** Rejected: `infrastructure.yml` has already written the drop-in by then, so the parent domain's token lands on the compromisable Host and is overwritten a play later. A window is not containment.
- **Give the agent tier its own reverse proxy rather than caddy.** Rejected: one HTTPS terminator per Host is the fleet's shape, and a second one would need its own ACME state, its own vhost convention and its own Ingress Gate.
