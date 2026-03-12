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
- apps.yml: Cloudflare API token and port 853 configuration

## Config Validation

Before executing any playbook, the CLI validates required config values from `config.toml` and exits with an error if any are missing or empty.

| Playbook           | Required config keys                                    |
| ------------------ | ------------------------------------------------------- |
| bootstrap.yml      | `admin_user_name`, `ssh_port`                           |
| hardening.yml      | (none)                                                  |
| infrastructure.yml | `admin_user_name`, `domain`, `tailscale_authkey`        |
| apps.yml           | `admin_user_name`, `domain`, `cloudflare_dns_api_token` |
| other playbooks    | `admin_user_name`, `domain`                             |

Example error output:

```
✗ Missing required config values:
✗   'admin_user_name' is required. Run: auberge config set admin_user_name <VALUE>
✗   'domain' is required. Run: auberge config set domain <VALUE>
Error: 2 required config value(s) missing in config.toml
```

## Automatic Dependency Resolution

When `--tags` is provided **without** `--playbook`, the CLI auto-resolves which playbooks need to run based on the tags:

- App tags (e.g., `paperless`, `baikal`) trigger `infrastructure.yml` first (full run, idempotent), then `apps.yml` with only the specified tags
- Infrastructure tags (e.g., `tailscale`, `caddy`) run only `infrastructure.yml` with those tags
- Mixed tags run both playbooks in order: infrastructure first, then apps

Specifying `--playbook` explicitly **bypasses** auto-resolution — only the named playbook runs.

## Options

| Option              | Description                                                                        | Default               |
| ------------------- | ---------------------------------------------------------------------------------- | --------------------- |
| -H, --host HOST     | Target host                                                                        | Interactive selection |
| -p, --playbook PATH | Playbook path (bypasses auto-resolution when combined with `--tags`)               | Interactive selection |
| -C, --check         | Run in check mode (dry run)                                                        | false                 |
| -t, --tags TAGS     | Comma-separated tags to run (auto-resolves playbooks when `--playbook` is omitted) | All tasks             |
| --skip-tags TAGS    | Comma-separated tags to skip                                                       | None                  |
| -f, --force         | Skip confirmation prompts (for CI/CD)                                              | false                 |

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

# Auto-resolve: deploys full infrastructure first, then paperless from apps.yml
auberge ansible run --host myserver --tags paperless

# Explicit playbook: runs only apps.yml with the tag (no infra auto-deploy)
auberge ansible run --host myserver --playbook ansible/playbooks/apps.yml --tags paperless

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
  1. Get your ssh_port: auberge config get ssh_port
  2. Log into your VPS provider dashboard (IONOS, etc.)
  3. Add firewall rule: Allow TCP on your ssh_port
  4. Save and confirm the rule is active

Without this, you'll be locked out after SSH port change!
```

## Apps Playbook Warnings

When running apps.yml:

**Cloudflare API Token**:

- Zone → Zone → Read
- Zone → DNS → Edit
- Set with: `auberge config set cloudflare_dns_api_token your-token`

**Port 853 (DNS over TLS)**:

- Must be opened in VPS provider firewall
- Required for Blocky DoT functionality

## Related Commands

- [auberge deploy](../deploy.md) - Deploy apps (recommended for app deployments)
- [auberge ansible bootstrap](bootstrap.md) - Bootstrap a new VPS
- [auberge select playbook](../select/playbook.md) - Select playbook interactively

## See Also

- [Ansible Playbooks](../../core-concepts/ansible.md)
- [Bootstrap Process](../../getting-started/bootstrap.md)
- [Applications](../../applications/README.md)
