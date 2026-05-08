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

Every deploy runs `hardening → infrastructure → apps` in that order. Hardening (firewall, fail2ban, kernel) is mandatory and untagged. To skip it, use [`auberge ansible run`](ansible/run.md) directly.

The CLI shows the resolved plan before running (suppress with `-f`):

```
Execution plan:
  Host: prod (203.0.113.10)
  → hardening
  → infrastructure
  → apps (tags: paperless, freshrss)
```

## DNS verification

After each app's playbook run (not in `--check`):

- **Tailnet-only apps** (apps declaring `{app}_tailscale_ip`): verified automatically against Blocky on the tailnet IP (UDP/53).
- **Public apps**: opt-in via `--verify-public-dns`. `1.1.1.1` is queried; the A record must match `ansible_host`. Opt-in because Cloudflare propagation can lag.

A mismatch aborts the deploy:

```
DNS mismatch for paperless.example.com: queried 100.64.1.2, expected 100.64.1.2, got [1.2.3.4]
```

?> App names are derived from roles in `apps.yml`. Run `auberge deploy` without args to see the multi-select list.
