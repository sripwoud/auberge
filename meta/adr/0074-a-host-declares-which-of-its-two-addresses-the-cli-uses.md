# ADR-0074: A Host declares which of its two addresses the CLI uses

## Status

Accepted, 2026-09-01. Slice 4 of 4 from the #780 tailnet-transport design, and the only one with observable behaviour. Built on ADR-0067's `Route` seam, #785's host identity, and ADR-0070's include binding. Changes the trust model: the CLI's path to a Host can now be a private overlay rather than the public internet.

## Decision

**`hosts.toml` grows `prefer_tailnet: bool`. It is a policy; `tailscale_ip` stays a fact.** Holding the fact never implies the decision — `vieille-auberge` caches `100.64.0.4` and must never be routed over it, so `host detect-tailscale-ip` writes the address and neither sets nor clears the policy.

`services::route::resolve` answers the policy, and every ssh, scp, rsync and ansible connection already collapses through it (ADR-0067), so one function moved and four command families followed.

### Strict, with no fallback

A Host carrying the policy is reached at its `tailscale_ip` or not at all. No warning-and-continue: `auberge-backup.service` runs `backup sync … --quiet`, so a warning in the nightly path is structurally invisible, and a silent fall back to the public address _is_ the incident #780 comes from — a hand-added `ProxyJump` stanza rerouted every automated path for ten days.

`tailscale_ip` is the policy's **precondition**, checked by `Host::validate` at every roster write, before either file is touched. That is what dissolves the bootstrap chicken-and-egg: `tailscale` is a role in `ansible/playbooks/infrastructure.yml`, so a Host joins the tailnet during a run — a fresh Host has no fact, cannot carry the policy, and the run that enrolls it goes over the public address.

A **stale** `tailscale_ip` gets no guard. An address that moved is indistinguishable from a Host that is down until you try it, so the guarantee is that it fails loud and names its own fix. `services::ssh::unreachable_error` — the one place a transport failure is worded — appends `auberge --via public host detect-tailscale-ip <name>` whenever the route was a CGNAT address. Keyed on `100.64.0.0/10` rather than on a flag threaded down from the resolver: the range is the fact, and reading it there also covers a Route that reached the tailnet some other way.

### `--via <public|tailnet>`

A global flag, set once in `main`, read only by the resolver. `--via public` is the recovery route; `--via tailnet` proves one before it is declared.

Global because half a route is worse than none: ssh going public while ansible still tries the tailnet is the same divergence in miniature. It reaches ansible's own `ansible_host` because the roster→Inventory conversion goes through `resolve` like everything else.

Two things it deliberately does not do:

- **It never reaches the generated ssh include**, which is written from the new `route::declared`. That file outlives its command, and ADR-0070 regenerates it on _every_ roster write — so `auberge --via public host edit x` would otherwise republish every alias on the public address permanently, leaving interactive `ssh x` on a route nobody declared. The flag meant to work around #780's divergence would have reintroduced it.
- **It does not pass silently when it decides nothing.** `--via` on a command that connects to no host exits non-zero saying so.

### The Inventory's two addresses are named apart

`services::inventory::HostVars::ansible_host` answered two questions, and this slice made them diverge. It is now `public_address`; where the CLI connects is `Host::connect_address`, resolved once at the roster→Inventory conversion.

Three consumers needed the public one and would have silently taken the route:

| consumer                                          | reads             | had it followed the route            |
| ------------------------------------------------- | ----------------- | ------------------------------------ |
| `dns set-all` (`discover_hosts_with_ips`)         | A record value    | publishes `100.64.0.1` in public DNS |
| `deploy`'s public DNS check (`app_verify_config`) | expected A record | asserts the record it just broke     |
| fail2ban's `ignoreip` (`hosts_ignoreip_var`)      | peer allowlist    | drops the public address from it     |

The third is the sharpest, and is why `route::peer_addresses` exists: it lists **both** of a Host's addresses. Once peers arrive over the tailnet, ignoring only the address in use would make the `--via public` recovery route the bannable one, at exactly the moment it is needed.

And one consumer needed the route and was taking the declaration: `sync music` built its rsync destination from `ansible_host`. It was the one command that would have been left behind.

## Why

`tailscale_ip` has been on `Host` since #755 and nothing could connect over it. The public path to a Host is the one an attacker can also see; the overlay is authenticated at the transport, so moving the CLI onto it removes the public sshd from the path for every automated command.

### Why a per-Host declaration and not a global setting

The fleet is not uniform in this. `vieille-auberge` is the rollback surface, and routing it over a tailnet served by the box you are rolling back _from_ is a circular dependency with bad timing. A global switch cannot express "auberge yes, vieille-auberge never".

### Why the policy is validated at the write and not only at the prompt

`hosts.toml` is hand-edited. A prompt guard validates one operator's keystrokes; the invariant is about the file. `host edit` still declines to _offer_ the choice to a Host with no cached address — not because that is where the rule lives, but because offering a choice the write will reject throws away the seven answers already typed.

### Why the unused-`--via` check runs after the command

Whether a command routes to a Host is only knowable once it has tried. The alternative is a static list of routing subcommands, which is precisely the kind of list a new command forgets to join — the defect ADR-0070 removed from the include. The cost is stated below.

## What it costs

- **A `--via` that decided nothing exits non-zero after work that already succeeded.** `auberge --via public host edit x` performs the edit and then fails. The exit code says the invocation was wrong, which it was; a CI wrapper reading it as "the edit failed" would be wrong in the safe direction. Accepted over the alternative: a flag that reads as applied and was not, which on `--via public` means believing you moved off a route you are still on.
- **`--via tailnet` is fleet-wide, so it fails if _any_ roster Host lacks a cached address**, not only the target — the roster→Inventory conversion resolves every entry. Deliberate: applying the override at a second, target-only site would put the policy in two places, which is what ADR-0067 spent a slice removing. `--via public`, the recovery direction, can never fail this way.
- **`resolve` is now fallible**, and roughly twenty construction sites propagate it. The one case it catches is a hand-edited roster whose policy outlived its fact.
- **The route to the backup target depends on a service the backup target hosts.** headscale runs on auberge. Bounded by measurement: no node key on this tailnet expires, so a running `tailscaled` keeps its WireGuard peers through a headscale outage — losing the route needs headscale down _and_ a `tailscaled` restart in the same window. `--via public` is the way out, and it belongs in the runbook rather than being discovered under pressure.
- **A stale binary rewrites `hosts.toml` wholesale.** ADR-0069's `unknown` map protects fields this binary does not know; it does not protect `prefer_tailnet` from a binary that predates it, which would drop the field and, under strict resolution, quietly move the route to the public address rather than erroring. General fix tracked separately.
- **`route::public_address` is a one-line accessor over `Host::address`.** It exists to make the question askable by name at a call site that must not take the route, and to keep `one_route_to_a_host.rs`'s scan over `services/inventory.rs` intact. A passthrough that pays for itself in the DNS table above.

## Rollout

auberge (`100.64.0.1`) first, then ruche once steady. **vieille-auberge never.**

## Alternatives considered

- **Derive the policy from `tailscale_ip`'s presence.** Rejected: it is the exact conflation this ADR names. `vieille-auberge` holds one and must stay public, and `detect-tailscale-ip` would become a command that silently changes routing.
- **Fall back to the public address when the tailnet does not answer.** Rejected: the nightly backup runs `--quiet`, so the fallback would be invisible, and an invisible route change is the incident.
- **Probe the tailnet address before returning a Route.** Rejected: a round trip on every command to answer a question the connection itself answers a moment later, and a probe that passes says nothing about the connection that follows.
- **Keep `ansible_host` as the connection address and give DNS a new field.** Rejected: it leaves the ambiguous name attached to the majority of readers, so the next reader still has to know which meaning is in force. Naming both for their answers is the same move ADR-0067 made by nesting a `Route` inside `InventoryHost`.
- **Make `--via` a per-command flag on the commands that route.** Rejected: it is the same enumeration the post-hoc check avoids, with the added failure that a command missing the flag silently keeps the declared route.
- **Warn instead of failing on an unused `--via`.** Rejected for the reason the whole slice is strict: `--quiet` exists, and a warning nobody sees is a no-op with extra steps.

## References

- Issue #780 — the settled design record; #787 — this slice.
- ADR-0067 — the `Route` seam; ADR-0070 — the include binding this consumes.
- ADR-0069 — the `unknown` map, and the loss it does not cover.
- CONTEXT.md — **Route**, **SSH Include**, **Host**.
