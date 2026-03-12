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

| Option            | Description                                                | Default               |
| ----------------- | ---------------------------------------------------------- | --------------------- |
| `apps...`         | Space-separated app names to deploy (positional, variadic) | Interactive selection |
| `-H, --host HOST` | Target host                                                | Interactive selection |
| `-C, --check`     | Dry-run mode (ansible check mode, no changes applied)      | false                 |
| `--all`           | Deploy all apps                                            | false                 |
| `-f, --force`     | Skip confirmation prompt                                   | false                 |

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

`--check` runs in ansible check mode: tasks are evaluated but no changes are applied to the host. Useful for previewing what a deployment would change.

See [Check Mode](../deployment/check-mode.md) for output interpretation.

## Related Commands

- [auberge ansible run](ansible/run.md) - Low-level playbook execution with full options
- [auberge backup create](backup/create.md) - Create backup before deploying

## See Also

- [Check Mode](../deployment/check-mode.md)
- [Running Playbooks](../deployment/running-playbooks.md)
- [Applications](../applications/README.md)
