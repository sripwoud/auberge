# auberge deploy

Deploy apps to a host

## Synopsis

```bash
auberge deploy [apps...] [OPTIONS]
```

## Alias

`auberge dp`

## Description

Deploys one or more self-hosted applications to a target host using Ansible. Infrastructure dependencies are resolved automatically — you do not need to specify playbooks or manage layer ordering.

A confirmation prompt shows the execution plan before running (unless `--force` is passed).

App names are derived from roles defined in `apps.yml`.

## Options

| Option            | Description                                                | Default               |
| ----------------- | ---------------------------------------------------------- | --------------------- |
| `apps...`         | Space-separated app names to deploy (positional, variadic) | Interactive selection |
| `-H, --host HOST` | Target host                                                | Interactive selection |
| `-C, --check`     | Dry-run mode (ansible check mode, no changes applied)      | false                 |
| `--all`           | Deploy all apps                                            | false                 |
| `-f, --force`     | Skip confirmation prompt                                   | false                 |
| `--verbose`       | Increase ansible output verbosity                          | false                 |

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
Deploying: paperless freshrss
Host: prod

? Confirm? [y/N]:
```

Use `--force` to skip this prompt.

### Infrastructure Dependencies

Infrastructure layers required by selected apps are resolved automatically. You do not need to reference `infrastructure.yml` or any other playbook.

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
