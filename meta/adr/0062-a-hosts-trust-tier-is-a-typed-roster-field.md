# ADR-0062: A Host's trust tier is a typed roster field, checked against the shipped policy

## Status

Accepted, 2026-09-01. Decided and implemented in #767. Extends [ADR-0055](./0055-the-tailnet-runs-a-tag-based-acl-policy.md); corrects the validation seam #767 specified in light of [ADR-0061](./0061-a-first-acl-policy-is-rolled-out-in-two-stages.md).

## Decision

**`hosts.toml` gains `tailnet_tag`, a closed type over ADR-0055's four tiers, and it is the Host's only declaration of trust.** It is spelled bare (`tailnet_tag = "agent"`); `TailnetTag::acl_tag` adds the `tag:` prefix at the one boundary that needs it.

**The check is static, and it is not the one the issue asked for.** #767 said "reject a value the deployed policy's `tagOwners` does not define". Three distinct questions were hiding in that sentence, and they are answerable at three different moments:

| check                                             | question                          | when                         |
| ------------------------------------------------- | --------------------------------- | ---------------------------- |
| `validate_tag` (`commands::headscale`)            | a well-formed tag?                | any `--tags`, unchanged      |
| `TailnetTag`                                      | one of the fleet's tiers?         | `hosts.toml` parse — **new** |
| headscale's `TagExists`, translated by `tag_node` | does the _loaded_ policy name it? | `nodes tag`, unchanged       |

Only the middle one is answerable while writing a roster entry, so it is the one the field gets. `tests/headscale_acl_policy.rs` holds `TailnetTag::ALL` and the shipped `policy.hujson`'s `tagOwners` equal **in both directions**, so the type cannot drift from the policy and neither can widen alone. Widening the tailnet's trust vocabulary stays a decision made twice (ADR-0046).

**`tailscale_advertise_tags` is deleted, and a fence keeps it deleted.** Node-side advertisement was a second writer for a fact the pre-auth key already owns.

## Why

### Why not `tags`

`hosts.toml` `tags` are already load-bearing as Ansible inventory groups — `all.children.<tag>` (`services/ansible_runner.rs`), read by `when: "'<x>' in group_names"` (`playbooks/apps.yml`, `hermes.yml`). Deriving the tier from that list would make "which roles run here" and "what this Host may reach on the network" the same declaration, so adding an Ansible group would silently move a Host's trust. The current roster makes the collision concrete: `auberge` carries `["hermes"]`, which is a role selector and not a tag any `tagOwners` defines, while `ruche` carries `["agent"]`, which matches `tag:agent` by coincidence alone.

This is not the alternative [ADR-0058](./0058-config-answers-per-host.md) rejected. That one proposed reading trust tiers _out of_ `tags` to gate roles — one field answering two questions, which is the trap this ADR also avoids. Here the tier is its own field and gates nothing: it never reaches Ansible at all.

### Why not the deployed policy

`SetNodeTags` gates every tag on `polMan.TagExists`, and `TagExists` returns `false` while `pm.pol == nil` (ADR-0061). So the deployed policy answers "no" to a perfectly legal tier whenever no policy is loaded — which was true of this tailnet for a day, and is true of any tailnet before its first policy deploy and any Host enrolled before its control plane exists. A declaration validated against that gate would be refused for reasons that say nothing about the declaration.

It is also the wrong coupling. `auberge host add` edits a local file; answering `TagExists` needs SSH to the control-plane Host, so the roster edit would fail when that Host is down, unreachable, or — in the case that motivated the field, `ruche`'s virgin run — not yet built.

The runtime gate is not lost. It keeps the owner #771 gave it: `tag_node`'s `TAG_NOT_IN_POLICY` translation, which fires at the moment the tag is actually applied and explains that the _policy_ is what is missing.

### Why the repo file is a legitimate authority

`policy.hujson` is a `files/` asset, not a template: no Jinja, no per-Host variance, byte-identical on every tailnet. Which tiers exist is therefore a static property of the repo, and pinning a type to it is pinning to a fact rather than to a snapshot of remote state.

### Why it does not reach Ansible

Nothing node-side needs it. Tags are server-forced through the pre-auth key, `tailscale_advertise_tags` is gone, and the consumer is CLI-side: #768's auto-mint reads the target's tier to stamp `--tags` on the key it mints. Plumbing it into the inventory would recreate the `tags`/`group_names` coupling this field exists to avoid.

## What it costs

**A typo in `tailnet_tag` fails every command, not just the next tailnet one.** `HostManager::load_hosts` is on the path of nearly everything, so an unparseable roster stops all of it. Accepted: the refusal names the file, the line, and all four legal values (`` unknown variant `yolo`, expected one of `trusted`, `data`, `agent`, `standby` ``), and a roster that cannot be read is not a state to continue from.

**The field is optional, so an untagged Host is still expressible.** It has to be — the roster predates the field, and `lechuck` and `pixel-9a` have no `hosts.toml` entry and never will. The mitigation is visibility rather than a required field: `auberge host list` grew a `TIER` column showing `-` when unset, and `host add`/`host edit` prompt for it, because four of five nodes carrying no tag (#765) is precisely what stayed invisible while the tier was an argument typed once at enrollment.

**A fifth tier is now two edits, not one.** `TailnetTag` and `policy.hujson`'s `tagOwners`, in either order — the fence fails until both move. That is the point: a tier the CLI accepts but the policy never names is a `hosts.toml` value headscale will refuse, and a tier the policy names but the type does not is one no Host can be declared with.

## Alternatives considered

- **Validate against the deployed policy, as #767 specified.** Rejected above: it answers a different question, fails on an unloaded policy, and couples a local file edit to remote reachability.
- **Query the deployed policy and fall back to the static set when unreachable.** Rejected: a check that degrades to the static one is the static one, plus a network round trip and a second failure mode.
- **A `[hosts.<name>]` config key.** Rejected on ADR-0058's own rule: the per-Host config table carries which of the fleet's _answers_ a Host withdraws or replaces, and a trust tier is not an answer to a gate — nothing conditions on it. ADR-0059 rejected the same move for `<app>_tailscale_ip` from the other direction.
- **A free-text `String` validated by `validate_tag`.** Rejected: that function checks tag _syntax_, deliberately, so it can serve `add-key -t` for nodes outside the roster. It would accept `tag:agnet`, and the roster would carry it until a `nodes tag` months later refused it.
- **Require the field.** Rejected: it makes every existing `hosts.toml` unparseable to gain a check the `TIER` column already surfaces.
