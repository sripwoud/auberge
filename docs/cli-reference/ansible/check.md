# auberge ansible check

Run Ansible playbook in check mode (dry run)

## Synopsis

```bash
auberge ansible check [OPTIONS]
```

## Alias

`auberge a c`

## Description

Runs an Ansible playbook in check mode (dry run) to preview changes without applying them. Equivalent to `auberge ansible run --check`.

Check mode shows what would change without making actual modifications to the target host.

## Options

| Option              | Description               | Default               |
| ------------------- | ------------------------- | --------------------- |
| -H, --host HOST     | Target host               | Interactive selection |
| -p, --playbook PATH | Playbook path             | Interactive selection |
| -f, --force         | Skip confirmation prompts | false                 |

## Examples

```bash
# Interactive check
auberge ansible check

# Check specific playbook
auberge ansible check --host myserver --playbook ansible/playbooks/apps.yml

# Skip confirmations
auberge ansible check --host myserver --playbook ansible/playbooks/bootstrap.yml --force
```

## Understanding Check Mode Output

Ansible check mode output indicates:

**changed**: Task would make changes (yellow in terminal)
**ok**: Task would not change anything (green)
**skipped**: Task skipped due to conditions

Example output:

```
TASK [baikal : Install baikal] ****************************
changed: [myserver]  # Package would be installed

TASK [baikal : Configure baikal] **************************
ok: [myserver]  # Configuration already correct

TASK [baikal : Restart service] *****************************
skipped: [myserver]  # Conditional not met
```

## Limitations

Some tasks cannot be fully checked:

- Commands with no idempotency checks
- Tasks that query external state
- Shell scripts without check mode support

## Related Commands

- [auberge ansible run](run.md) - Run playbook normally
- [auberge ansible bootstrap](bootstrap.md) - Bootstrap a new VPS

## See Also

- [Ansible Playbooks](../../core-concepts/ansible.md)
- [Deployment](../../deployment/README.md)
