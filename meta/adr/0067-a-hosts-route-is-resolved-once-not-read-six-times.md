# ADR-0067: A Host's route is resolved once, not read six times

## Status

Accepted, 2026-09-01. Slice 1 of 4 from the #780 tailnet-transport design. Pure seam: no observable behaviour changes in this slice. #785–#787 build on it; #787 is the only slice with a real policy. Partially superseded by ADR-0070 (the ssh include's `Port` and `User` directives).

## Decision

**`services::route::Route` is the one resolved answer to "how do we reach this Host" — address, port, user, an optional key path, and an alias.** `Host` stays the declaration (`hosts.toml`); a consumer that needs to connect takes a `Route`, produced by `route::resolve(host, key_path)`.

`resolve` is unconditional in this slice: it always returns the Host's public address. No `prefer_tailnet`, no policy — #787 adds that. The seam exists now so that when it does, one function changes rather than six read sites.

**Six modules previously read `Host::address` directly** to decide how to connect: `services/ssh.rs`, `services/ssh/transport.rs`, `services/ssh_include.rs`, `services/ansible_runner.rs` (via `InventoryHost`), `services/inventory.rs` (ansible's `ansible_host` var), and `commands/sync.rs`. Each now takes a `Route` instead:

- `SshTransport` and `LiveSshSession` hold a `Route` and a `become_method: &str`, not a `Host`. `become_method` stays separate — it is an escalation policy (#776), unrelated to where the CLI connects.
- `ansible_runner::InventoryHost` holds `route: Route` instead of duplicate `address`/`port`/`user` fields, so its own internal reads (`known_hosts` lookups, the written inventory YAML) go through the same type rather than a second copy of the same three primitives.
- `services::inventory::convert_xdg_host_to_inventory_host` resolves a `Route` before populating `ansible_host`/`ansible_port`.
- `services::ssh_include::render` resolves a `Route` per Host for the generated config's `HostName` line. `Port`/`User`/`IdentityFile` stay Host-derived — the issue is address provenance, not the whole stanza. **`Port` and `User` superseded by [ADR-0070](./0070-the-ssh-include-is-regenerated-by-the-roster-write.md)**: they are connection directives too, and leaving them on the declaration would let #787 publish a tailnet address with the public host's port. `IdentityFile` stays Host-derived, for the reason given under _Why `key_path` is optional_.

**The declared exception is bootstrap** (`commands/ansible.rs::run_ansible_bootstrap`): a virgin host has no tailnet yet and no verified `hosts.toml` identity of its own — the operator confirms an IP at a prompt — so it constructs its `Route` literal directly rather than resolving one, marked `#780`. It is the _only_ hand-built `Route` literal in the crate: `run_ansible_bootstrap`'s two steady-state siblings (`run_auto_resolved`, `run_single_playbook`) and `commands/deploy.rs::run_deploy` build an `InventoryHost` from `services::inventory::Host` too, but from an already-provisioned Host, so they go through the new `services::inventory::Host::route()` — a companion to the pre-existing `Host::ssh_target`, for the same reason: neither call site holds a `crate::hosts::Host` to hand `route::resolve`.

`SshTransport::new`/`first_contact` (and `LiveSshSession`'s) return `Result`, refusing a `Route` whose `key_path` is `None` at construction — the one place a `None`-key Route would be a bug in the caller, not a legitimate use.

**Fenced by `tests/one_route_to_a_host.rs`**, a sibling to `ssh_stays_in_the_transport.rs` (that one fences command _spelling_; this one fences address _provenance_). It scans a curated list of the six consumer modules for a direct `Host::address` read, asserts the resolver itself still reads one (so the scan is not vacuous), and — line-anchored against the literal itself rather than a whole-file substring check, since `commands/ansible.rs` and `commands/deploy.rs` legitimately build several `InventoryHost`s — asserts that every `Route { .. }` literal in those two modules is the one declared exception, still marked `#780`.

## Why

`tailscale_ip` has been stored on `Host` since #755 but nothing could connect over it — every ssh, rsync, scp and ansible run went to the public address regardless. Before a routing policy could exist, "the address" needed to stop being six independent answers computed ad hoc at each call site; otherwise #787 would have had to teach six modules the same policy, and a seventh call site added later would silently keep the old behaviour.

### Why not `hosts.rs`

`hosts.rs` legitimately reads `Host::address` for serialization (`toml`/`serde_yaml`) and for the `host add|edit|list|show` CLI surface — those are declaration-level concerns, not routing decisions, and must keep showing the Host's own declared address forever, including after #787. Putting the resolver there would make the fence exempt exactly the module it most needs to constrain.

### Why `InventoryHost` holds a `Route` rather than duplicate fields

`InventoryHost` (the DTO `ansible_runner::run_playbook`/`run_bootstrap` take) had its own `address`/`port`/`user`, copied from `Host` at each construction site. Left alone, a textual fence over "did this read `Host::address`" could not distinguish a legitimate read of `InventoryHost`'s own field from a read of `Host`'s — both are spelled `host.address` at a variable named `host`. Nesting a `Route` inside `InventoryHost` turns every internal read into `host.route.address`, which cannot collide with the pattern the fence forbids, and it means #787's policy reaches ansible plays (`ansible_host` included) the same way it reaches ssh, for free.

### Why `key_path` is optional

Not every consumer that needs a `Route` needs a real, filesystem-checked identity file: ansible resolves its own connection independently of this seam, and the ssh include's `IdentityFile` line has its own tilde-preserving logic that a resolved `PathBuf` would break. Making the field mandatory would force those callers to resolve (and risk failing on) a key they never use — `services::inventory::convert_xdg_host_to_inventory_host` runs on every `hosts_ignoreip_var()` call, so a new failure mode there would newly break every ansible command for a Host whose key happens to be missing.

## What it costs

- Every `LiveSshSession`/`SshTransport` construction site (~20) now computes a `Route` before constructing the session, and propagates its now-fallible constructor with `?` — mechanical, but real churn.
- `become_method` is threaded as a parameter independent of `Route`, paired with it by convention at each call site rather than by the type system — two Hosts juggled in the same function (`auto_mint_for`'s target/coordinator pair) rely on the caller not swapping one Host's route with another's escalation string. Accepted rather than folded into `Route`: `become_method` is a fixed policy (#776), not a routing concern, and the mismatch is a same-function copy-paste away from being caught by any test that exercises the two Hosts differently.
- `route.alias` is currently always `host.name` and read nowhere except by test/format code — declared now so #785's `HostKeyAlias` migration has a field to fill rather than a new struct to design under review pressure.

## Alternatives considered

- **Ban `Host::address` reads crate-wide**, exempting only `hosts.rs`. Rejected: `commands/host.rs`'s edit-prompt default and `commands/backup.rs`'s human-facing ssh hints read `Host::address` for declaration display, not a connection decision, and must keep doing so even after #787 — routing them through `Route` would make them silently show a tailnet address once that policy exists, which is wrong for a value the operator is editing or about to type by hand.
- **Fold `become_method` into `Route`.** Rejected: it is an escalation policy fixed independently in #776, orthogonal to where the CLI connects — folding it in would make `Route` answer two different questions.
- **A crate-wide fence banning bare `Route { .. }` literals** (a sibling check to the address one). Rejected for this slice: test fixtures across `services::ssh`/`services::ssh::transport` construct `Route` values directly, and a blanket ban would have made ordinary test setup indistinguishable from the one real exception. The bootstrap exception is instead checked narrowly, by name, the way `DECLARED_HAND_BUILT_TRANSPORT` checks `commands/sync.rs` in the sibling fence.

## References

- Issue #780 — the settled design record (routing model, failure semantics, host identity, slice order).
- Issue #784 — this slice.
- ADR-0058 — `[hosts.<name>]` host-scoped config, the same "declaration vs. resolved view" split this ADR extends to connectivity.
- CONTEXT.md — **Host**.
