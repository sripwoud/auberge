# ADR-0061: A first ACL policy is rolled out in two stages, and carries its own acceptance

## Status

Accepted, 2026-09-01. Decided and executed in #765. Discharges the acceptance [ADR-0055](./0055-the-tailnet-runs-a-tag-based-acl-policy.md) gated on #738 and #738 never ran. Depends on `tag-node` (#769/#771).

## Decision

**A tailnet's first policy is deployed twice: a bridge, then the real one.** The bridge declares the full `tagOwners` set and nothing else — its ACL stays `{"src": ["*"], "dst": ["*:*"]}`. Loading it changes no reachability, so there is no window in which the tailnet is partitioned; what it changes is that `headscale nodes tag` starts working. Every already-enrolled node is tagged against the bridge, and only then does the real default-deny policy deploy. The bridge is a transient working-tree state, never committed — `tests/headscale_acl_policy.rs` fails on it, so it cannot be.

**The policy carries a `tests` block, and the block is the acceptance.** Three `PolicyTest` entries assert what the four ACL rules are for: `tag:agent` reaches the resolver on 53 and is denied the data host's SSH and HTTPS; `tag:trusted` reaches all three; `tag:data` reaches `tag:trusted` and is denied `tag:agent`. `headscale policy check -f` evaluates them against the live fleet without applying anything, and `pm.RunTests()` re-runs them on every policy load — so a later rule change that reopens a confined path stops the policy instead of shipping quietly.

**A tier named by a test is pinned to being non-empty.** An unresolvable `src` is a test _failure_, not a skip. `tag:standby` is deliberately untested for that reason: `vieille-auberge` is the fleet's only member and is slated for retirement, and emptying the tier would otherwise stop the policy loading.

## Why

ADR-0055's rollout gate said to verify flows before and after and to "enroll `ruche` only once the policy is live, so no flat-tailnet window ever exists." #738 was closed with every acceptance box unticked, the policy merged but never deployed, and `ruche` enrolled into a tailnet that was still allow-all — the window the gate existed to prevent, held open for a day, with an agent host inside it.

#765 was filed to close that, and prescribed the obvious repair: tag the four untagged nodes first, deploy second. **That order is impossible.** `SetNodeTags` (`hscontrol/state/state.go`) gates every tag on `polMan.TagExists(tag)`, and `TagExists` returns `false` while `pm.pol == nil`. With no policy loaded, every `nodes tag` is rejected. The asymmetry that hid this: a tag stamped on a pre-auth key is applied _unchecked_ at registration, which is why `ruche` carried `tag:agent` on a tailnet with no policy at all and looked like a counter-example.

So the two orders each fail on their own: tag-first cannot start, and deploy-first is a default-deny tailnet in which no rule matches anybody, because every rule is keyed on a tag no node yet carries — including the DNS carve-out, whose `dst` is `tag:data:53`. The bridge is the third order, and it has neither failure.

The `tests` block answers the other half. #738's acceptance was three prose assertions in an issue, and prose does not run; the issue was closed with them unverified and the tailnet flat. The same three assertions as `PolicyTest` entries are executed by headscale's own filter compiler on every load. During #765's rollout, mutating one to assert `tag:agent` _may_ reach `tag:data:59865` was rejected with `expected ALLOWED, got DENIED` — which is how we know the block is evaluated rather than parsed and ignored, and how the confinement was proven before the flag day rather than after it.

### What it costs

**Two deploys and a working-tree swap instead of one deploy.** The bridge has to reach the Host through the same role that ships the real policy, so it is applied by pointing `AUBERGE_DEV=1` at a worktree holding the bridge, then restoring. Accepted: the alternative is a partition of unbounded length, since it lasts until a human finishes tagging.

**A test's `src` tier may not be emptied.** Retiring the last node in a tested tier now stops the policy loading — a loud failure, but a surprising one if the connection is not documented. Named here, and the untested `tag:standby` is the case it would have bitten.

**The bridge is a real, if brief, allow-all policy on disk.** It is strictly no weaker than the state it replaces (no policy at all, which is also allow-all), so it widens nothing; it is called out because "we deployed an allow-all ACL" reads alarming without that context.

## Alternatives considered

- **Deploy the real policy, then tag quickly.** Rejected: it is a full partition — DNS included — for as long as the tagging takes, and tagging is four interactive confirmations deep. That the control path survives (SSH and Ansible reach Hosts on public addresses, never the tailnet) makes it recoverable, not acceptable.
- **Grant untagged nodes a fallback rule in the real policy**, so an untagged tailnet degrades instead of partitioning. Rejected: it makes the default-deny flip conditional on a rule whose only purpose is to be removed later, and a fallback nobody removes is the policy.
- **Tag by re-enrolling each node with a stamped pre-auth key**, which bypasses `TagExists` entirely. Rejected: it is `tailscale logout` on a phone and two laptops to avoid one extra deploy, and it changes node identity to fix a metadata field.
- **Keep the acceptance as a runbook of `nc` probes.** Rejected: it was the runbook that failed. #765's own note records the original probes targeting `100.64.0.1:22` and HTTPS-on-bare-IP, both of which fail for reasons unrelated to any ACL and duly reported "blocked" against a fully open tailnet. A probe that cannot distinguish a policy from a closed port is not evidence — and the replacement harness reproduced the same class of error once more (a missing `nc` binary read as "blocked") before growing the positive controls that now gate it.
- **Assert the confinement in `tests/headscale_acl_policy.rs` instead.** Rejected as a replacement, kept as a complement: that fence reads the policy file's shape and cannot compile a filter or see a node, so it can prove the rule is written and not that the tailnet enforces it.
