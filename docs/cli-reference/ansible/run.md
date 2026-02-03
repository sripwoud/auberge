# auberge ansible run

Run Ansible playbooks on hosts

## Synopsis

```bash
auberge ansible run [OPTIONS]
```

## Alias

`auberge a r`

## Description

Executes Ansible playbooks on target hosts. Supports check mode (dry run), tag filtering, and interactive host/playbook selection.

**Important warnings are shown for**:

- bootstrap.yml: Provider firewall configuration required
- apps.yml/auberge.yml: Cloudflare API token and port 853 configuration

## Options

| Option              | Description                           | Default               |
| ------------------- | ------------------------------------- | --------------------- |
| -H, --host HOST     | Target host                           | Interactive selection |
| -p, --playbook PATH | Playbook path                         | Interactive selection |
| -C, --check         | Run in check mode (dry run)           | false                 |
| -t, --tags TAGS     | Only run tasks with these tags        | All tasks             |
| -f, --force         | Skip confirmation prompts (for CI/CD) | false                 |

## Examples

```bash
# Interactive mode (select host and playbook)
auberge ansible run

# Run specific playbook on specific host
auberge ansible run --host myserver --playbook ansible/playbooks/apps.yml

# Dry run (check mode)
auberge ansible run --host myserver --playbook ansible/playbooks/apps.yml --check

# Run with specific tags
auberge ansible run --host myserver --playbook ansible/playbooks/apps.yml --tags freshrss,baikal

# Skip confirmations (for automation)
auberge ansible run --host myserver --playbook ansible/playbooks/bootstrap.yml --force
```

## Bootstrap Warnings

When running bootstrap.yml, you'll see:

```
IMPORTANT: Provider Firewall Configuration Required
Before running bootstrap, ensure your VPS provider's firewall
allows your custom SSH port (separate from UFW on the VPS)

Required steps:
  1. Get your SSH_PORT: mise env | grep SSH_PORT
  2. Log into your VPS provider dashboard (IONOS, etc.)
  3. Add firewall rule: Allow TCP on your SSH_PORT
  4. Save and confirm the rule is active

Without this, you'll be locked out after SSH port change!
```

## Apps Playbook Warnings

When running apps.yml or auberge.yml:

**Cloudflare API Token**:

- Zone → Zone → Read
- Zone → DNS → Edit
- Set with: `mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN`

**Port 853 (DNS over TLS)**:

- Must be opened in VPS provider firewall
- Required for Blocky DoT functionality

## Related Commands

- [auberge ansible check](check.md) - Run playbook in check mode
- [auberge ansible bootstrap](bootstrap.md) - Bootstrap a new VPS
- [auberge select playbook](../select/playbook.md) - Select playbook interactively

## See Also

- [Ansible Playbooks](../../core-concepts/ansible.md)
- [Bootstrap Process](../../getting-started/bootstrap.md)
- [Applications](../../applications/README.md)
