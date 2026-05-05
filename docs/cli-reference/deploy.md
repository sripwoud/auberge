# auberge deploy

Deploy apps to a host with automatic hardening

## Synopsis

```bash
auberge deploy [apps...] [OPTIONS]
```

## Alias

`auberge dp`

## Description

Deploys one or more self-hosted applications to a target host using Ansible. Every deploy automatically runs hardening first, then resolves infrastructure dependencies — you do not need to specify playbooks or manage layer ordering.

A confirmation prompt shows the resolved execution plan (hardening → infrastructure → apps) before running (unless `--force` is passed).

To skip hardening, use `auberge ansible run` directly.

App names are derived from roles defined in `apps.yml`.

## Options

| Option                 | Description                                                | Default               |
| ---------------------- | ---------------------------------------------------------- | --------------------- |
| `apps...`              | Space-separated app names to deploy (positional, variadic) | Interactive selection |
| `-H, --host HOST`      | Target host                                                | Interactive selection |
| `-C, --check`          | Dry-run mode (ansible check mode, no changes applied)      | false                 |
| `--all`                | Deploy all apps                                            | false                 |
| `-f, --force`          | Skip confirmation prompt                                   | false                 |
| `--verify-public-dns`  | After each app's playbook run, verify the public A record resolves to the host IP (queries `1.1.1.1`; opt-in because Cloudflare propagation can be slow) | false |

## Examples

```bash
# Interactive: select app(s) and host
auberge deploy

# Deploy a single app, interactive host select
auberge deploy paperless

# Deploy multiple apps to a specific host
auberge deploy paperless freshrss --host prod

# Deploy all apps to a host
auberge deploy --all --host prod

# Dry run: preview changes without applying
auberge deploy paperless --check

# Deploy without confirmation prompt (for automation)
auberge deploy --all --host prod --force

# Deploy and verify public DNS after each app run
auberge deploy paperless --host prod --verify-public-dns
```

## Behavior

### Interactive Mode

Running `auberge deploy` without arguments opens:

1. A multi-select list of available apps (from `apps.yml`)
2. A host selector (if `--host` is not specified)

### Confirmation Prompt

Before executing, the CLI displays the execution plan:

```
Execution plan:
  Host: prod (203.0.113.10)
  → hardening
  → infrastructure
  → apps (tags: paperless, freshrss)

Proceed with deployment? [y/N]:
```

Use `--force` to skip this prompt.

### Execution Order

Every deploy runs in this order:

1. **Hardening** — firewall, fail2ban, kernel hardening (always, no tags)
2. **Infrastructure** — Caddy, Tailscale, DNS (auto-resolved dependencies)
3. **Apps** — only the selected applications (filtered by tags)

To skip hardening or run arbitrary playbooks, use [`auberge ansible run`](ansible/run.md).

### Check Mode

`--check` runs in ansible check mode: tasks are evaluated but no changes are applied to the host. Useful for previewing what a deployment would change. DNS verification is skipped in check mode.

See [Check Mode](../deployment/check-mode.md) for output interpretation.

### DNS Verification

After each successful app playbook run (not in `--check` mode), `auberge deploy` can verify that the app's FQDN resolves to the expected IP:

- **Tailnet-only apps** (those with a `{app}_tailscale_ip` config key): verified automatically. Blocky is queried directly at the tailscale IP (UDP/53) and the returned A record must match that same IP.
- **Public apps**: opt-in via `--verify-public-dns`. `1.1.1.1` is queried and the returned A record must match the host's `ansible_host` IP. This flag is opt-in because Cloudflare propagation can be slow immediately after a deploy.

If verification fails, the deploy is aborted with an actionable diagnostic:

```
DNS mismatch for paperless.example.com: queried 100.64.1.2, expected 100.64.1.2, got [1.2.3.4]
```

## Related Commands

- [auberge ansible run](ansible/run.md) - Low-level playbook execution with full options
- [auberge backup create](backup/create.md) - Create backup before deploying

## See Also

- [Check Mode](../deployment/check-mode.md)
- [Running Playbooks](../deployment/running-playbooks.md)
- [Applications](../applications/README.md)
