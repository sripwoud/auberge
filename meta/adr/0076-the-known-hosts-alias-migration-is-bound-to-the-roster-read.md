# ADR-0076: The known_hosts alias migration is bound to the roster read, not the roster write

## Status

Accepted, 2026-09-01. Closes #800, a defect in [ADR-0070](./0070-the-ssh-include-is-regenerated-by-the-roster-write.md)'s slice of the #780 tailnet-transport design. Supersedes that ADR's binding of the `known_hosts` migration to `save_hosts`, and the cost it recorded as benign. Blocks nothing else: `prefer_tailnet` is still enabled on no Host.

## Decision

**`HostManager::read_roster` is the one read of `hosts.toml`, and it carries every Host's already-verified host key onto its `HostKeyAlias` from the same slice.** `load_hosts` is a thin resolver over it, and `get_host`, `list_hosts_filtered` and all four mutation paths go through `load_hosts`.

`save_hosts` no longer migrates. The include regeneration stays bound to the write ([ADR-0070](./0070-the-ssh-include-is-regenerated-by-the-roster-write.md)); only the migration moves.

The read and the migration sit in one private function, `read_roster(config_path, known_hosts)`. Both paths are parameters for the reason `write_roster`'s are: it is what lets the binding be asserted against temp files instead of the developer's `$HOME`. Making that possible took one change in `services::known_hosts` — `ssh-keygen -F` takes `-f <file>`, so the trust store became a parameter and the migration became observable at all.

**A pre-#785 key is looked for under both spellings ssh accepts**, most specific first: `[address]:port`, then the bare `address`. `legacy_target` returned only the first for a non-default port; `legacy_targets` returns both.

**`inspect` and `forget` keep ssh's default file.** They neither write nor sit inside a binding a test has to observe, and threading a path through them would be churn with no assertion to serve.

## Why

`HostKeyAlias` and the entries it resolves against were on two clocks, and nobody wound the second one.

#785 made every CLI connection send `-o HostKeyAlias=<name>`. #786 bound the migration that creates those `known_hosts` entries to a roster mutation. Upgrading is not a roster mutation, so the first run of a binary carrying #785 finds nothing under the alias:

```
$ ssh -o HostKeyAlias=auberge -o BatchMode=yes -p 59865 sripwoud@159.195.111.227 true
Host key verification failed.
$ ssh -o BatchMode=yes -p 59865 sripwoud@159.195.111.227 true
sripwoud@159.195.111.227: Permission denied (publickey).
```

Same target, same second. The alias is the whole cause: without it the connection reaches authentication.

### Why the documented fallback did not hold

[ADR-0070](./0070-the-ssh-include-is-regenerated-by-the-roster-write.md) recorded the gap and called it benign — an unmigrated Host would "accept-new under the alias like a fresh host, exactly once." That relies on `StrictHostKeyChecking accept-new`, which exists only in the generated include's `Host <name>` stanza. The transport connects to `user@<address>` and never by name, so the stanza never matches and the option never applies. ssh's default `ask` governs instead, and in a non-interactive context `ask` is a hard failure.

It is the same split #780 exists over, one layer down: the path a human tests by hand (`ssh auberge`, matching the stanza) is the one that works, and the CLI's own path is the one that does not.

### Why two legacy spellings

Moving the trigger was necessary and not sufficient. On the first real run the migration fired, walked all three Hosts, and wrote nothing — because `legacy_target` built `[159.195.111.227]:59865` and the key is filed under the bare `159.195.111.227`.

ssh writes the bracketed form for a non-default port but does not require it. OpenSSH 10.5p1, connecting to port 59865 with no user config in play:

```
debug1: Host '135.125.107.230' is known and matches the ED25519 host key.
debug1: Found key in /home/sripwoud/.ssh/known_hosts:14
```

Two of the three Hosts on this fleet are stored that way. A bare-address entry that ssh honours _is_ the trust the operator has, so the migration has to carry it; looking under one spelling left the fix running and doing nothing, with no output to say so. Order is ssh's own: where both entries exist ssh checks the port-keyed one, so the alias inherits that key rather than a leftover from a port-22 era.

The general shape is worth naming, because the first fix had it: a migration whose trigger is right and whose lookup is wrong fails exactly like one that never ran, and both are silent. The verification that caught it was running the binary against the real trust store and re-running the issue's own command — not the unit tests, which agreed with the bug.

### Why the read

The trigger has to be an event the upgraded binary actually has. Reading the roster is that event, and it is the only one every affected command shares — `deploy`, `ansible run`, `sync`, `headscale`, `backup` and the nightly `auberge-backup.service` all resolve a Host before they connect. Binding at `load_hosts` rather than at `main` also keeps the trigger where the data is, so a command that reads the roster from a future entry point inherits it instead of remembering it.

It closes a second case #786 never covered, for the same reason: a Host bootstrapped _after_ the upgrade. Bootstrap TOFUs the address, nothing writes the roster afterwards, and every later command sends an alias nothing has migrated. On the read, the next command repairs it.

### Why not `accept-new` in the transport

It is the fix this failure invites, and #780 settled against it: it makes `Host key verification failed` go away by trusting whatever answers under an alias the CLI has never seen, inside a change whose entire purpose is knowing where traffic goes. `tests/the_alias_follows_the_binary.rs` bans it in the senders, because without the ban the next regression can be "fixed" that way and every other assertion still passes.

The ansible leg is the honest exception, and it is not an endorsement: `ansible/inventory.yml` sets `StrictHostKeyChecking=accept-new` for the connection ansible makes itself, so that leg would have blind-trusted the alias rather than failed. The migration running first is what keeps it from doing so.

### Why the fence is a sibling, not an extension

`the_include_follows_the_roster.rs` fences whether the published route was rewritten; this fences whether the identity that route is checked under is trusted. Widening an existing fence's subject is how a scan starts passing vacuously — the same call [ADR-0067](./0067-a-hosts-route-is-resolved-once-not-read-six-times.md) and [ADR-0070](./0070-the-ssh-include-is-regenerated-by-the-roster-write.md) make.

`tests/the_alias_follows_the_binary.rs` asserts five directions: the set of modules that put `HostKeyAlias` on the wire is closed; each of them still sends it; `read_roster` calls the migration; nothing else does; and no sender sets `StrictHostKeyChecking`. All five were mutation-tested and each fails exactly one assertion — an undeclared sender, a sender that stops sending, a deleted migration call, a command that migrates on its own, and a sender opting into TOFU. The behavioural half — that reading actually migrates — is asserted where it can be observed, in `hosts::tests::read_roster_migrates_every_hosts_known_hosts_alias`.

That every sender is _downstream_ of the read is left to the compiler rather than a scan: `HostsConfig` is private to `src/hosts.rs`, so a `Host` can only come from `load_hosts`.

## What it costs

- One `ssh-keygen -F` per Host per read in the steady state — the alias lookup hits and returns early. An unmigrated Host on a non-default port costs three (alias, bracketed, bare), once. `load_hosts` is called more than once by some commands, so the real figure is a small multiple. Measured against an ansible run it is noise; it is still work `auberge host list` did not do before.
- `load_hosts` now resolves `dirs::home_dir()` and shells out, so a command that only lists Hosts fails on a broken `ssh-keygen` where it used to succeed. Fail-fast, and consistent with `save_hosts` since ADR-0070, but it is a wider blast radius for the same class of breakage.
- The roster module's read path now touches `~/.ssh`, where before only its write path did. `hosts.rs` was already coupled to `services::known_hosts`; the coupling is now on both directions of the roster boundary.
- `read_roster` takes two paths its only caller immediately resolves from the environment. The indirection exists for the test, and is stated as such rather than dressed up as configurability.
- The migration is additive and never removes anything, so an alias entry outlives the roster entry that justified it. Unchanged from #785, and still the safe direction: a stale alias line is inert, where a missing one is an outage.

## Alternatives considered

- **Trigger at CLI startup, in `main`.** Rejected: `main.rs` is the clap tree and the dispatch `match` and nothing else, and a migration there fires for `completions` and `config path` alike while still needing the roster. The read is the same guarantee, scoped to the commands that have a Host.
- **Keep the migration on `save_hosts` as well.** Rejected as a second trigger for one invariant. Every mutation path reads before it writes, so the read already covers `add`, `edit`, `remove` and `rename`; the only thing the write would add is a migration for an alias declared seconds earlier by a command that then connects to nothing.
- **`StrictHostKeyChecking=accept-new` in the transport.** Rejected — see above, and #780.
- **Migrate inside `route::resolve`,** the tightest possible binding to the thing that carries the alias. Rejected: `resolve` is a pure function with unit tests that would then shell out against the developer's real trust store on every call, and the seam #780 built is about deciding an address, not about performing I/O.
- **Ship the migration as a one-shot `auberge` subcommand** and name it in the release notes. Rejected: it is the recovery that was already undiscoverable, promoted to a documented step. A fleet upgraded by a `mise` pin bump has no moment at which anyone reads release notes before `auberge-backup.service` next runs.

## References

- Issue #800 — the defect; #785 — the alias; #786 / [ADR-0070](./0070-the-ssh-include-is-regenerated-by-the-roster-write.md) — the binding this moves.
- Verified on the fleet: `auberge --via public headscale list-nodes`, the command #800 reproduces with, fails `Host key verification failed` before and returns the node list after.
- Issue #780 — the settled tailnet-transport design; #799 — the rollout this unblocks; #751 — the release that must not ship without it.
- CONTEXT.md — **Route**, **SSH Include**.
