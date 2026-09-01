# Tailnet Transport

By default the CLI reaches a host at the public `address` in its `hosts.toml` entry. A host that sets `prefer_tailnet = true` is reached at its `tailscale_ip` instead — by SSH, scp, rsync **and** Ansible, together.

```toml
[[hosts]]
name = "auberge"
address = "203.0.113.10"
tailscale_ip = "100.64.0.1"   # the fact
prefer_tailnet = true         # the policy
```

The two fields are separate on purpose. Caching an address never implies routing over it: `vieille-auberge` holds `100.64.0.4` and must stay on the public route, because it is the rollback surface. `auberge host detect-tailscale-ip` writes the address and never touches the policy.

## Enabling it

```bash
auberge host detect-tailscale-ip auberge   # caches tailscale_ip
auberge host edit auberge                  # answer yes to "Route over the tailnet address"
```

`auberge host list` shows a `ROUTE` column — `public` or `tailnet` — beside each host. The `ADDRESS` column keeps showing the declared public address either way.

> [!IMPORTANT]
> `prefer_tailnet` without a cached `tailscale_ip` is refused at every write, including a hand-edited `hosts.toml`. A host joins the tailnet during `infrastructure.yml`, so a fresh host has no address to route to and the run that enrolls it necessarily goes over the public one.

## No fallback

If the tailnet address does not answer, the command **fails**. It does not warn and retry over the public address.

That is deliberate. `auberge-backup.service` runs `backup sync … --quiet`, so a warning in the nightly path is invisible, and a silent route change is the failure this feature exists to prevent — a hand-added `ProxyJump` stanza once rerouted every automated path for ten days without anything reporting it.

A `tailscale_ip` that has gone stale gets no freshness check; it fails and names its own fix:

```
host auberge is unreachable over ssh at 100.64.0.1:22
Check the SSH key and network connectivity
This is auberge's tailnet address. If it is stale, refresh it with
`auberge --via public host detect-tailscale-ip auberge`; `--via public` routes one
command over the public address.
```

## `--via`: overriding the route for one command

```bash
auberge --via public backup sync --host auberge   # this run over the public address
auberge --via tailnet deploy --host ruche         # prove a route before declaring it
```

`--via` is global and applies to every transport the command uses, Ansible's `ansible_host` included — a flag that moved SSH but left Ansible behind would be worse than none.

| Behaviour                                                        | Why                                                                                                                                                           |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Does not change `~/.ssh/config.d/auberge.conf`                   | That file outlives the command and is regenerated on every roster write; a per-run override baked into it would leave `ssh <name>` on a route nobody declared |
| Exits non-zero if the command routed nowhere                     | A flag believed to have moved the route and did not is the failure mode this feature guards against. The command itself still runs                            |
| `--via tailnet` needs every roster host to have a `tailscale_ip` | The whole roster is resolved when the Inventory is built. `--via public`, the recovery direction, never fails this way                                        |

## Recovery

Headscale runs on `auberge`, so the tailnet route to the backup target depends on a service that target hosts. This is bounded rather than fixed: no node key on this tailnet expires, so a running `tailscaled` keeps its WireGuard peers through a Headscale outage. Losing the route needs Headscale down **and** a `tailscaled` restart in the same window.

When that happens, `--via public` is the way out — see [Cross-Host Migration](backup-restore/cross-host-migration.md#when-the-tailnet-route-is-down).

## What still uses the public address

Enabling `prefer_tailnet` moves connections only. These keep reading the declared public address, and must:

| Consumer                          | Reads             | Why                                                                    |
| --------------------------------- | ----------------- | ---------------------------------------------------------------------- |
| `auberge dns set-all`             | A record value    | A CGNAT address in public DNS resolves for nobody                      |
| `auberge deploy` public DNS check | Expected A record | Same record, verified                                                  |
| fail2ban `ignoreip`               | Peer allowlist    | Both addresses are listed, so `--via public` recovery cannot be banned |

See [ADR-0074](https://github.com/sripwoud/auberge/blob/master/meta/adr/0074-a-host-declares-which-of-its-two-addresses-the-cli-uses.md).
