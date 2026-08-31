# auberge deploy

Deploy one or more apps to a host. Hardening runs first automatically, then infrastructure dependencies are resolved. Alias: `auberge dp`.

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

## Substrate Apps

Substrate Apps (Caddy, Blocky, Headscale) are declared in `infrastructure.yml`, not `apps.yml`, so they are not valid `deploy` targets — they deploy on every `auberge deploy <app>` run instead. To push one alone:

```bash
auberge ansible run -t blocky
```

## DNS verification

After each app's playbook run (not in `--check`):

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

?> App names are derived from roles in `apps.yml`. Run `auberge deploy` without args to see the multi-select list. Roles declared in `infrastructure.yml` are rejected with a pointer to [`auberge ansible run`](cli-reference/ansible/run.md) `-t <role>`.
