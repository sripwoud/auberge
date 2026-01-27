# Running Playbooks

## Usage

```bash
auberge ansible run                           # Interactive
auberge ansible run --host H --playbook P    # Explicit
auberge ansible run --tags TAG               # By tags
```

## Examples

```bash
# Full stack (skip bootstrap if already done)
auberge ansible run --playbook playbooks/auberge.yml --skip-tags bootstrap

# Specific layer
auberge ansible run --playbook playbooks/apps.yml

# Specific apps
auberge ansible run --tags radicale,freshrss

# Single component
auberge ansible run --tags caddy
```

## Common Scenarios

**First deployment:**

```bash
auberge ansible bootstrap auberge --ip IP
auberge ansible run --playbook playbooks/auberge.yml --skip-tags bootstrap
```

**Update single app:**

```bash
auberge ansible run --tags radicale
```

**Update infrastructure:**

```bash
auberge ansible run --tags infrastructure
```

## Options

- `-v, -vv, -vvv` - Verbose output (debugging)
- `--force` - Skip confirmation (CI/CD)
- `--dry-run` / `check` - Preview without applying

## Error Handling

Ansible stops on first failure. Fix issue and re-run (idempotent - safe to retry).

**Connection issues:** See [SSH Problems](troubleshooting/ssh-problems.md)

## Best Practices

- Backup before updates: `auberge backup create`
- Use check mode for unfamiliar changes
- Verify services after: `ssh ansible@host "systemctl status app"`
