# ADR-0050: A remote CLI's contract is pinned to the release it was verified against

## Status

Accepted, 2026-08-29.

## Decision

**A module that drives another project's command line names the release it was read off, and a fence fails the build when the App Version moves past it.**

`src/commands/headscale.rs` declares `VERIFIED_CLI_VERSION`; `tests/headscale_cli_contract.rs` asserts it equals the App Version `ansible/playbooks/headscale.meta.yml` pins (ADR-0017). A Renovate bump therefore lands red until someone reads the new release's flags and JSON and moves the const.

The const sits next to the code it is true of, not in the fence, so a reader changing a command string is looking at the version that string was verified under. The fence imports it rather than scraping the source (ADR-0046), and asserts the pin it read is a real declared App Version — a comparison between two lookups passes vacuously when either lookup finds the wrong thing.

This is not a test of the remote binary. Nothing in a `cargo test` reaches a Host, and that is the point: the check is not "does the contract hold" but "has anyone looked since it moved".

## Why

`auberge headscale add-user` had never worked against the deployed binary, and the suite was green.

`preauthkeys create --user` takes a `uint` user ID. The command passed the username. Cobra rejects the flag value, so `add-user` created the user, then died minting its key — the failure lands _after_ the mutation, so the operator is left with a user, no key, and no enrollment instructions. Never observed live only because zero users have ever existed on either instance (#510).

The audit that fix required found two more defects of the same kind, in the two commands the issue did not name:

| Command              | Assumed                                 | headscale 0.29.3 emits                |
| -------------------- | --------------------------------------- | ------------------------------------- |
| `preauthkeys create` | `--user <username>`                     | `--user <uint id>`                    |
| `nodes list -o json` | protojson keys: `givenName`, `lastSeen` | proto keys: `given_name`, `last_seen` |
| `users list -o json` | `[]` when empty                         | `null` when empty                     |

The node fields are the sharpest of the three, because the assumption was reasonable and wrong: headscale serialises its protobuf types with Go's `encoding/json` over the generated struct tags, not with protojson. The tags carry the _proto_ field names, so every node listing failed on `missing field givenName`, and every `uint64` arrives as a JSON number rather than protojson's string. `null` has the same origin — `printListOutput` hands `encoding/json` a nil Go slice, and a nil slice marshals to `null`, not `[]`. `nodes list` happened to handle that; `users list` did not, so a `remove-user` picker on a fresh instance reported a parse error instead of "no users".

Three breaking changes to one contract, none of them noticed, because Renovate walked headscale 0.25 → 0.29 across four PRs (#427/#436/#437/#445) and nothing in the repo related the pinned release to the code that talks to it. The tests were not absent — they mocked the ssh layer, which answers whatever the test staged. **A mock pins the caller, never the callee.** Where the callee is a versioned binary someone else ships, the only thing the repo can pin is who last looked.

### What the fence cannot do, and what covers the rest

It cannot verify a flag. Moving the const is a human reading `--help`; the fence only refuses to let that step be skipped.

What it pairs with is the seam. Every remote call now goes through a `&dyn SshSession` function — `create_user`, `mint_preauth_key`, `list_users`, `list_nodes`, `destroy_user` — so the exact command line reaching the Host is an assertion (ADR-0047: the seam is a runtime argument).

One of those is the _sequence_, not a call: `add_user` runs create-then-mint, because the defect was never in either call but in the value threaded between them, and the entry point holding that threading builds its own `LiveSshSession` and can never be reached from a test. `mint_preauth_key`'s `u64` already makes the original regression a type error; the sequence makes it a test failure too, including for a future signature that stopped being a `u64`. The parse tests are fed **verbatim 0.29.3 output**, tabs and `omitempty` gaps included, rather than JSON written from the shape the code wanted. Reverting each of the three defects fails a test, which is the property that was missing: the previous fixtures were hand-written in protojson's dialect and so agreed with the bug.

### The prose is a third coupling, and the weakest one

Amended 2026-08-30 (#729). The contract above was written over two couplings: the command lines auberge spells and the JSON they print. `register` added a third — auberge matches `node not found in registration cache` in headscale's stderr to tell an aged-out enrollment from a real failure, and quotes `registerCacheExpiration`'s 15 minutes back to the operator.

It is the weakest of the three because it is the only one no `--help` shows and no `-o json` prints. A flag that changes breaks loudly: cobra rejects it and the command fails. Reworded prose breaks silently in the safe direction — the translation stops firing, the operator gets the raw text back, and every test stays green, because the mocks stage the string the code already believes in. **A mock pins the caller, never the callee** holds here with nothing to soften it.

So the pin is what carries it: `REGISTRATION_CACHE_MISS` and `registerCacheExpiration` are named in the fence's message alongside the `--help` surfaces, and moving `VERIFIED_CLI_VERSION` means re-reading all three kinds. What is deliberately not built is a fallback — an unrecognised stderr surfaces verbatim rather than under a guess, so the failure mode of a reword is a worse message, never a wrong one.

### A guard applied on one of two paths is not applied

The audit turned up one defect that is not a version contract at all. `remove-user` validates the username it is _given_ and could not validate the one it is _told_: the interactive picker takes a name from headscale's own store — where an OIDC claim or `users rename` can put anything — and handed it straight to a `sudo` command line the remote login shell parses.

Fixed by quoting at the command builder rather than by widening `validate_username`, because the two disagree about what a legal name is and the store's answer is the one that binds: an OIDC-provisioned `alice@example.com` is a real user the charset guard would have made unremovable. Validation stays where it belongs — rejecting bad input auberge is asked to _create_ — and quoting covers every interpolation regardless of provenance.

### Why the id comes from `users create`, not a second `users list`

The issue proposed resolving the ID with a follow-up `users list -o json`. The create response already carries the user it just made, so the key is minted from that instead. One round trip fewer is incidental; what matters is that nothing can fail _between_ the mutation and the key that mutation exists to mint — which is the precise shape of the failure being fixed.

### Not covered: the JSON auberge itself prints

`list-users --output json` and `list-nodes --output json` re-serialise the wire types, so auberge's own structured output currently leaks headscale's protobuf-JSON — `created_at: {seconds, nanos}` where `docs/cli-reference/` promised an ISO string. The docs are corrected here to describe what is emitted, and the coupling is left standing: a headscale release changing its proto changes auberge's `--output json` contract, which ADR-0004 says is auberge's to define and ADR-0043 says should stop at an adapter. Deliberately out of scope for a fix whose subject is the _remote_ contract, and filed rather than folded in.
