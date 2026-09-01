# auberge deploy

Deploy one or more apps — or a whole Host composition — to a host. Hardening runs first automatically, then infrastructure dependencies are resolved. Alias: `auberge dp`.

```bash
auberge deploy [apps...] [OPTIONS]
```

## Options

| Option                | Description                                    | Default     |
| --------------------- | ---------------------------------------------- | ----------- |
| `apps...`             | App names (positional, variadic)               | Interactive |
| `-H, --host HOST`     | Target host                                    | Interactive |
| `--all`               | Deploy every app                               | `false`     |
| `-C, --check`         | Dry-run (ansible check mode)                   | `false`     |
| `-f, --force`         | Skip confirmation prompt                       | `false`     |
| `--verify-public-dns` | Verify each app's public A record after deploy | `false`     |

## Examples

```bash
auberge deploy                                          # interactive
auberge deploy paperless freshrss --host prod
auberge deploy --all --host prod -f                     # CI/CD
auberge deploy paperless --check                        # preview
auberge deploy paperless --host prod --verify-public-dns
auberge deploy ruche --host ruche                       # a whole composition
```

## Execution order

Every deploy runs `hardening → infrastructure → apps` in that order. Hardening (firewall, fail2ban, kernel) is mandatory and untagged. To skip it, use [`auberge ansible run`](cli-reference/ansible/run.md) directly.

The CLI shows the resolved plan before running (suppress with `-f`):

```
Execution plan:
  Host: prod (203.0.113.10)
  → hardening
  → infrastructure
  → apps (tags: paperless, freshrss)
```

## Composition targets

A name outside the `apps.yml` roster resolves to the standalone playbook of that name and runs after Substrate:

```
Execution plan:
  Host: ruche (203.0.113.42)
  → hardening
  → infrastructure
  → ruche
```

`ruche` is the agent tier's composition — machine-user identity, the OpenCode runtime and the aoe dashboard. A composition run pulls `infrastructure.yml` in for the same reason an app does: the dashboard is unreachable until Caddy holds a certificate for its zone ([ADR-0075](https://github.com/sripwoud/auberge/blob/master/meta/adr/0075-the-agent-tier-deploys-as-one-guarded-composition.md)).

> [!IMPORTANT]
> Every entry on `ruche.yml` — and on `aoe.yml` and `opencode.yml` — is guarded on `when: "'agent' in group_names"`, so the Host must carry `tags = ["agent"]` in `hosts.toml` or **the run is a green no-op that installs nothing**. Check it with `auberge host list`; the TAGS column must show `agent`. This is the `hosts.toml` `tags` field (an ansible group, deciding which roles run) — _not_ `tailnet_tag`, which happens to take the same word for the ACL tier and decides what the Host may reach on the tailnet. They are unrelated fields ([ADR-0062](https://github.com/sripwoud/auberge/blob/master/meta/adr/0062-a-hosts-trust-tier-is-a-typed-roster-field.md)).

Two playbooks are deliberately not deploy targets, because each is a lifecycle operation rather than a convergence:

| Playbook          | Run it with                                                       | Why not `deploy`                                                      |
| ----------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------- |
| `bootstrap`       | [`auberge ansible bootstrap`](cli-reference/ansible/bootstrap.md) | connects as root on port 22 before the ansible user exists            |
| `remove-radicale` | `auberge ansible run -t remove-radicale`                          | a teardown; prepending two convergence plays to it is the wrong order |

A name that is _both_ an apps.yml role and a standalone playbook (calibre, immich, gokapi, hermes) keeps going through the roster, which is where those apps have always deployed from.

## Substrate Apps

Substrate Apps (Caddy, Blocky, Headscale) are declared in `infrastructure.yml`, not `apps.yml`, so they are not valid `deploy` targets — they deploy on every `auberge deploy <app>` run instead. To push one alone:

```bash
auberge ansible run -t blocky
```

## DNS verification

After each app's playbook run (not in `--check`). A run's apps are its tags where it has them, and its roster where it does not — so a composition is checked through the apps on it:

- **Tailnet-only apps** (apps declaring `{app}_tailscale_ip`): the tailnet's Blocky is queried on UDP/53 and must answer with the app's own address. Blocky is the host whose config answers [`blocky_subdomain`](configuration/host-scoped-config.md), at the `tailscale_ip` that [`auberge host detect-tailscale-ip`](cli-reference/host/detect-tailscale-ip.md) cached for it — a different host from the app's as soon as the fleet grows.
- **Public apps**: opt-in via `--verify-public-dns`. `1.1.1.1` is queried; the A record must match `ansible_host`. Opt-in because Cloudflare propagation can lag.

A mismatch aborts the deploy:

```
DNS mismatch for essaim.example.com: queried 100.64.0.1, expected 100.64.0.9, got [100.64.0.1]
```

So does not knowing which host answers for the tailnet — a tailnet-only app's records live in that Blocky or nowhere, so an unanswerable check is a failed one:

```
Cannot verify essaim.example.com on the tailnet: 2 Hosts answer `blocky_subdomain` (auberge, ruche), but the tailnet has one resolver (ADR-0052); withdraw the gate on the others with `[hosts.<name>] blocky_subdomain = ""`
```

?> App names are derived from roles in `apps.yml`, plus the standalone playbooks above. Run `auberge deploy` without args to see the multi-select list of apps. Roles declared in `infrastructure.yml` are rejected with a pointer to [`auberge ansible run`](cli-reference/ansible/run.md) `-t <role>`.
