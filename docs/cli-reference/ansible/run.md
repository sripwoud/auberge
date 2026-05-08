# auberge ansible run

Run an Ansible playbook on a target host. Alias: `auberge a r`.

```bash
auberge ansible run [OPTIONS]
```

## Options

| Option                | Description                                                                | Default     |
| --------------------- | -------------------------------------------------------------------------- | ----------- |
| `-H, --host HOST`     | Target host                                                                | Interactive |
| `-p, --playbook PATH` | Playbook path (bypasses auto-resolution when combined with `--tags`)       | Interactive |
| `-C, --check`         | Dry run                                                                    | `false`     |
| `-t, --tags TAGS`     | Comma-separated tags (auto-resolves playbook when `--playbook` is omitted) | All tasks   |
| `--skip-tags TAGS`    | Comma-separated tags to skip                                               | None        |
| `-f, --force`         | Skip confirmation prompts (CI/CD)                                          | `false`     |

?> **Auto-resolution**: when `--tags` is set and `--playbook` is omitted, app tags (e.g. `paperless`) trigger a full `infrastructure.yml` run first (idempotent), then `apps.yml` with only those tags. Pass `--playbook` to bypass.

## Examples

```bash
auberge ansible run                                                      # interactive
auberge ansible run --host my-vps --tags paperless                       # auto-resolves infra + apps
auberge ansible run --host my-vps --playbook ansible/playbooks/apps.yml --tags freshrss,baikal --check
auberge ansible run --host my-vps --skip-tags navidrome -f               # CI/CD
```

## Required config keys per playbook

The CLI validates `config.toml` before running and exits with the missing keys.

| Playbook             | Required keys                                           |
| -------------------- | ------------------------------------------------------- |
| `bootstrap.yml`      | `hostname`, `admin_user_name`, `ssh_port`               |
| `hardening.yml`      | —                                                       |
| `infrastructure.yml` | `admin_user_name`, `domain`, `tailscale_authkey`        |
| `apps.yml`           | `admin_user_name`, `domain`, `cloudflare_dns_api_token` |
| other                | `admin_user_name`, `domain`                             |

## Common tags

List all tags for a playbook with `cd ansible && ansible-playbook playbooks/apps.yml --list-tags`.

| Tag                                                   | Scope                    |
| ----------------------------------------------------- | ------------------------ |
| `bootstrap` / `hardening` / `infrastructure` / `apps` | Layer                    |
| `ssh` / `ufw` / `fail2ban` / `kernel_hardening`       | Hardening component      |
| `caddy` / `apt` / `bash` / `tailscale`                | Infrastructure component |
| `<app-name>` (e.g. `baikal`, `freshrss`, `paperless`) | Single app               |
| `security` / `network` / `storage` / `web`            | Category                 |

!> **bootstrap.yml**: configure your VPS provider firewall to allow your custom `ssh_port` _before_ running, or you'll be locked out. **apps.yml**: requires `cloudflare_dns_api_token` (Zone:Read + DNS:Edit) and port 853/tcp open in the provider firewall (Blocky DoT).

<details>
<summary>Check mode limitations</summary>

Tasks that depend on previous tasks may report failures in check mode that wouldn't occur in real execution (e.g. copying a file into a directory the previous task only _would have_ created). `command`/`shell` modules always show "changed" — add `check_mode: no` for read-only commands. Handlers are notified but not executed.

</details>
