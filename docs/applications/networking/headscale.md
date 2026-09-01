# Headscale

Self-hosted Tailscale coordination server. Keeps device metadata on your own infrastructure. [Upstream docs](https://headscale.net).

- **URL**: `hs.{domain}` (must be public — clients contact it before joining the tailnet)
- **STUN**: 3478/udp (embedded DERP relay, region 999)
- **Data**: `/var/lib/headscale` (SQLite DB + noise keys)

## Deploy

```bash
auberge ansible run --tags headscale
```

## Required config

| Key                      | Purpose                                                                                                                                                                          |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `headscale_subdomain`    | Subdomain for the control plane (e.g. `hs`) — the deploy gate: unset or [blanked per host](configuration/host-scoped-config.md), the role and its STUN firewall rule are skipped |
| `tailscale_login_server` | Headscale URL (e.g. `https://hs.example.com`) — passed to `tailscale up --login-server`. When set, nodes use Headscale instead of Tailscale SaaS.                                |

## Enrollment keys

A pre-auth key is one-shot and expires. The CLI mints one **per run** rather than storing it ([ADR-0063](https://github.com/sripwoud/auberge/blob/master/meta/adr/0063-a-pre-auth-key-is-minted-per-run-not-stored.md)): a run entering the `tailscale` role reads this host's `headscale nodes list`, and if the target is not there, mints a 10-minute key stamped with the target's [`tailnet_tag`](configuration/hosts.md) and injects it as `tailscale_authkey`. An already-enrolled target costs one listing and nothing else.

`tailscale_authkey` is therefore **not** required config. It stays a settable key for one case: bootstrapping the host that will _serve_ headscale, which has no coordinator to mint against. Set it there, or leave it unset everywhere else.

| The run finds…                       | What happens                                                                        |
| ------------------------------------ | ----------------------------------------------------------------------------------- |
| the target already enrolled          | nothing minted; the role skips `tailscale up`                                       |
| one host serving headscale           | a 10m key minted for the tailnet's single user, tagged with the target's tier       |
| **no** host serving headscale        | nothing minted; `tailscale_authkey` from `config.toml` stands (the first-host case) |
| several such hosts, or several users | the run stops — there is no operator mid-deploy to ask which                        |

!> A minted key overrides `config.toml`'s value, because the CLI's `-e` is appended after the config extra-vars file. A stale key left in config is inert on any host with a reachable coordinator.

## First-run sequence

```bash
auberge ansible run --tags headscale
auberge headscale add-user --host my-vps            # the tailnet's one user
auberge config set tailscale_login_server https://hs.example.com
auberge ansible run --tags tailscale                 # keys minted per run from here on
```

?> Existing tailnet services (Paperless, Bichon, Cockpit) keep working unchanged — same Tailscale client, same WireGuard data plane.

## Relay

Peers that cannot reach each other directly relay through a DERP node. The embedded server on this Host is region 999; Tailscale's public DERP map is merged in beside it so relayed traffic survives this Host going down. Clients pick their home region by measured latency (`tailscale netcheck`), so the self-hosted one wins where it is closest rather than by configuration. Relay traffic is end-to-end encrypted WireGuard; a DERP node forwards it without being able to read it.

| Role var                          | Default                                                  | Purpose                                          |
| --------------------------------- | -------------------------------------------------------- | ------------------------------------------------ |
| `headscale_derp_urls`             | `["https://controlplane.tailscale.com/derpmap/default"]` | Remote DERP maps merged with the embedded region |
| `headscale_derp_update_frequency` | `24h`                                                    | How often to re-fetch them                       |

!> Headscale fetches these URLs at startup and **refuses to start if one fails** — it stops at the first failure, with no per-URL tolerance. Refreshes after startup are retried with backoff and are not fatal. To fall back to the embedded region alone, empty `headscale_derp_urls`; like `headscale_derp_enabled` beside it, that is a role default edited in the repo, not a `config.toml` key.

## Tailnet DNS

Headscale pushes the host's [Blocky](applications/networking/blocky.md) as the tailnet's **global** nameserver, so every query from every enrolled device is filtered — not just `*.{domain}`. The address is discovered at deploy time from the host's own `tailscale status`; before the host is enrolled there is nothing to discover and the config falls back to `1.1.1.1`/`1.0.0.1`.

A deploy that finds no tailnet IPv4 on a Host whose config already serves one **fails** rather than rendering the public fallback over it — an unreachable `tailscaled` must not silently unfilter every enrolled client.

Verify from any node:

```bash
tailscale dns status        # Resolver should be the host's tailnet IP
```

`headscale_split_dns_target_ip` stays available for pointing one domain at a different resolver — set it in inventory or via extra-vars; it is not a `config.toml` key. It is not how filtering reaches a client.

?> Blocky is load-bearing for **all** tailnet DNS under this design, not only internal apps — see [ADR-0052](https://github.com/sripwoud/auberge/blob/master/meta/adr/0052-the-tailnets-global-resolver-is-the-hosts-blocky.md).

## Tailnet ACL policy

The tailnet runs a tag-based, default-deny policy ([ADR-0055](https://github.com/sripwoud/auberge/blob/master/meta/adr/0055-the-tailnet-runs-a-tag-based-acl-policy.md)). The role deploys `policy.hujson` beside `config.yaml` and points headscale at it with `policy.mode: file` — repo-owned, not a `headscale policy set` into the DB. Its presence flips the tailnet from allow-all to deny-by-default.

Trust is a tag; which node carries which is set by `auberge headscale add-key -t tag:...` at enrollment or by [`auberge headscale tag-node`](cli-reference/headscale/tag-node.md) afterwards, so the policy names tiers, never nodes.

A host in the roster **declares** its tier as the `tailnet_tag` field of its [`hosts.toml`](configuration/hosts.md) entry — one of the four below, refused at parse time if it is anything else (ADR-0062). Parsing checks the value against the CLI's own closed type; what ties that type to this file is a test holding the two sets equal, so the four stay one vocabulary. That is the declaration; the commands above are what applies it. Nodes outside the roster (`lechuck`, `pixel-9a`) have no entry and are tagged by command alone.

| Tag           | Example nodes     | May initiate to                         |
| ------------- | ----------------- | --------------------------------------- |
| `tag:trusted` | lechuck, pixel-9a | everything                              |
| `tag:data`    | auberge           | trusted/data/standby; never `tag:agent` |
| `tag:agent`   | ruche             | the global resolver on 53 only          |
| `tag:standby` | vieille-auberge   | trusted/data/standby; never `tag:agent` |

Every tag reaches the host's [Blocky](applications/networking/blocky.md) global resolver on 53 (ADR-0052) — the one tailnet path open to `tag:agent`. `tagOwners` lists are empty: only the admin CLI that mints the keys may apply these tags, so the policy carries no operator-specific username.

### Rolling out a first policy

A tailnet's first policy is deployed **twice** ([ADR-0061](https://github.com/sripwoud/auberge/blob/master/meta/adr/0061-a-first-acl-policy-is-rolled-out-in-two-stages.md)). Neither obvious order works: `tag-node` cannot run before a policy is loaded (see the warning above), and deploying the real policy onto an untagged tailnet matches nobody — including the DNS carve-out, whose `dst` is `tag:data:53` — so it partitions rather than confines.

1. Deploy a **bridge**: the full `tagOwners` set with the ACL left at `{"src": ["*"], "dst": ["*:*"]}`. Reachability is unchanged; `tag-node` starts working.
2. Tag every enrolled node into its tier.
3. Deploy the real policy.

Verify with a before/after probe run at each step. Step 1 must show **no** change at all — if it does, the bridge was not inert.

!> Only tailnet paths are at risk: the tailnet-only vhosts, Blocky on 53, and syncthing's peer path. SSH, `auberge deploy` and backup pulls reach Hosts on their **public** addresses (`hosts.toml`), so a policy mistake cannot cut the control path or the backups — rollback is always one deploy away.

!> A probe that reports "blocked" must be able to report "open" first. #738's acceptance was declared met by probes against `100.64.0.1:22` and HTTPS-on-bare-IP, which fail for reasons unrelated to any ACL and read as confinement on a fully open tailnet. Probe a port that is listening (the Host's real SSH port), and gate the run on a positive control.

### The policy's own tests

`policy.hujson` carries a `tests` block asserting what its rules are for. `headscale policy check -f <file>` evaluates it against the live fleet **without applying anything** — run it between steps 2 and 3:

```bash
scp policy.hujson auberge:/tmp/candidate.hujson
ssh auberge 'sudo headscale policy check -f /tmp/candidate.hujson'
```

The same tests re-run on every policy load, so a rule change that reopens a confined path stops the policy rather than shipping quietly.

!> A tier named as a test's `src` may not be emptied — an unresolvable `src` is a test failure, not a skip, and the policy stops loading. `tag:standby` is untested for exactly this reason.

Put a node in a tier:

```bash
auberge headscale add-key -t tag:agent      # at enrollment: mint a key stamped tag:agent
auberge headscale tag-node lechuck -t tag:trusted   # afterwards: set an enrolled node's tags
```

A node **never asserts its own tag.** `tailscale up --advertise-tags` is a node-side claim headscale validates against `tagOwners`, where a key-stamped tag is server-forced and applied unchecked — two writers for one fact, and a rejected claim lands as a silently invalid tag on the node record. The tailscale role's `tailscale_advertise_tags` was deleted for that reason and `tests/headscale_acl_policy.rs` keeps it deleted.

!> The two paths are not validated alike. A tag on a pre-auth key is applied unchecked, so a node can carry a tag no policy names; `tag-node` requires the tag to appear under `tagOwners` in a **deployed** policy and rejects it otherwise. Tagging nodes that are already enrolled therefore happens _after_ the policy is live, not before. `tag-node` also replaces a node's tag set rather than adding to it, and converts a user-owned node to a tag-owned one irreversibly.

## Migration from Tailscale SaaS

On each node: `tailscale logout`, then re-run `auberge ansible run --tags tailscale`. Verify with `tailscale status`.

!> The host's own tailnet IP changes when it re-enrols, and both Blocky's bind address and the nameserver Headscale pushes were rendered from the **old** one. Run the infrastructure play once more after the host has re-enrolled, or tailnet DNS stays pointed at an address that no longer answers:

```bash
auberge ansible run --tags infrastructure
```

## Backup

`auberge backup create --apps headscale` snapshots `/var/lib/headscale` (SQLite + keys). No external DB.
