# ADR-0063: A pre-auth key is minted per run, not stored

## Status

Accepted, 2026-09-01. Decided and implemented in #768. Consumes [ADR-0062](./0062-a-hosts-trust-tier-is-a-typed-roster-field.md)'s `tailnet_tag`; rests on [ADR-0057](./0057-a-hosts-name-is-its-remote-hostname.md) for node identity and [ADR-0051](./0051-the-headscale-gate-is-real-and-config-owned.md)/[ADR-0058](./0058-config-answers-per-host.md) for locating the control plane.

## Decision

**The CLI mints `tailscale_authkey` for each run that needs one and injects it as an extra-var. It is no longer a `required_key`, and `config.toml` holds at most a fallback.**

Four parts:

| part                                  | mechanism                                                                                                                                                                   |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **The mint is the gate**              | `infrastructure.meta.yml`'s `required_keys` drops to `[admin_user_name, domain]`. Preflight's early failure is preserved by the mint running before the play.               |
| **Only an enrolling run pays for it** | `required_keys::run_enters_role` answers "does this run enter `tailscale`" through the same role selection Preflight resolves keys with. An app deploy costs no round trip. |
| **The tier rides the key**            | The target's ADR-0062 `tailnet_tag` becomes the minted key's `--tags`, so a node lands in its ACL tier at enrollment rather than needing a `tag-node` afterwards.           |
| **The window is 10m**                 | Sized for the gap between the mint and `tailscale up`. `add-key`'s 24h is for a human carrying a key to a phone; the two callers have different windows.                    |

**Enrollment is read off the coordinator's node listing, not the target.** A target `headscale nodes list` already carries needs no key, and the flow stops before minting one. The name matched is `given_name`, which ADR-0057 ties to the roster name: a Host's name _is_ its remote hostname, and `tailscale_hostname` defaults to that hostname. This costs nothing — the session to the coordinator is open either way — and does not require the target to be reachable from the CLI's SSH seam at that moment.

**Zero coordinators falls back; several is an error.** These are different answers and `only_serving_host`, which collapses both to `None`, is deliberately not reused:

- **Zero** — no roster Host's config answers the headscale gate — is the first-host case. Bootstrapping the Host that will _serve_ headscale has nothing to mint from, so `config.toml`'s value stands and behaviour degrades to exactly what it was.
- **Several** is a roster the auto-mint cannot read. Picking one would mint a key for a tailnet the target is not joining, and no operator is present mid-deploy to ask.
- **Several headscale users** is the same stop, for the same reason.

**A declared-but-unmintable coordinator aborts.** The two halves of the design pull against each other here — "the mint is the gate" wants a failure to stop the run, "fall back when no headscale is reachable" wants it to proceed — and the split is between _declared absence_ and _runtime failure_. Only the first falls back.

**The registry gains `injected:`.** `tailscale_authkey` stays in `keys.yml`, marked as CLI-supplied. `tests/injected_keys.rs` holds the invariant that gives the flag teeth: an injected key is demanded by no Playbook Meta.

## Why

`tailscale_authkey` is one-shot with a TTL, and it was stored as a permanent required key. The two facts cannot both be respected: what Preflight demanded of every run, forever, was a string guaranteed meaningless the moment after its single use. Both authkey-enrolled nodes held `used=True` values. The runbook that produced `ruche` told the operator to paste a live secret into `config.toml` as a raw literal, diverging from the fleet's `!pa` convention — because there was nowhere better for it to go.

The majority of the tailnet already enrolled without a persisted credential: `lechuck`, `pixel-9a` and `vieille-auberge` came in through the CLI (`register_method=2`) with no stored key at all. The stored key was not how the fleet worked; it was how one path worked, demanded of every path.

### Why not warn and continue on an unenrolled node with no key

That produces a green run over a node that never joined the tailnet. The role's assert stays and hard-fails; only its `fail_msg` changed, because it named `config.toml` as the source and that is now wrong.

### Why `injected:` ended up meaning something other than #768 expected

The issue proposed the flag as a source for the answerability fence, which "derives CLI-injected names from Playbook Metas and needs a source for this one". That turned out to be moot: `tests/common`'s `registry_keys()` already answers _every_ `keys.yml` name, flag-blind, so `tailscale_authkey` was answerable throughout. The flag earned a better job instead — `tests/injected_keys.rs` holds that no Meta may declare an injected key, which is the invariant that makes the whole arrangement safe and which nothing else was checking.

### Why extra-vars order is load-bearing

`base_argv` emits `--extra-vars @<config file>`; a caller's pairs are appended after it as `-e key=value`, and ansible's last `-e` wins. That ordering is the only reason a fresh mint beats the spent `tailscale_authkey` a live `config.toml` still holds. Reversed, a stale credential silently wins over a good one and the play fails at `tailscale up` with no hint that a valid key was minted and discarded. `services::ansible_runner`'s `test_a_callers_extra_var_is_appended_after_the_config_file` pins it.

## Alternatives considered

- **Always mint, skip the enrollment check.** Rejected: every routine `deploy` would leave an unused credential row in headscale's store, and would need the coordinator reachable for runs that do not need it.
- **Read enrollment off the coordinator's `nodes list`.** Rejected after review, having first been built. It is cheaper — the coordinator session is open anyway — and `given_name` meets the roster name under ADR-0057. But it answers the wrong question, and the divergence is a dead end rather than a slow path: see the Decision above. The probe's extra round trip buys an answer that cannot be wrong.
- **A `headscale_user` config key.** Rejected: it swaps a persisted credential for a persisted required key, which is the shape this ADR exists to remove, and it would be a key that is right exactly once.
- **A headscale user per roster Host.** Rejected: registering with a tagged key nulls the node's `UserID`, so those users would own nothing the moment they were used.
- **Remove `tailscale_authkey` from the Key Registry entirely.** Rejected: the fallback needs a name config can carry, and the key is the canonical secret exemplar in ~20 tests across `key_registry`, `config`, `config_cmd`, `required_keys` and `playbook_meta`. `injected:` says what is true without rehoming them.

## Out of scope

`auberge headscale add-key` is unchanged. `lechuck` and `pixel-9a` have no `hosts.toml` entry and never will, so auto-mint cannot reach them by construction — the two paths serve disjoint populations.
