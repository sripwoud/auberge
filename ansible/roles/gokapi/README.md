# Gokapi Ansible Role

Deploys [Gokapi](https://github.com/Forceu/Gokapi) — a Go single-binary, self-hosted Firefox Send alternative — as a systemd service behind Caddy.

## Features

- Link-based file sharing with expiry by download count or time
- File Requests: per-URL upload links visible only to the creator
- Optional per-share password
- SQLite backend (default) or S3
- Optional OIDC integration

## Variables

| Variable              | Default                               | Description                                 |
| --------------------- | ------------------------------------- | ------------------------------------------- |
| `gokapi_install_path` | `/opt/gokapi`                         | Binary install directory                    |
| `gokapi_data_dir`     | `/var/lib/gokapi`                     | Persistent data + DB directory              |
| `gokapi_config_dir`   | `/var/lib/gokapi/config`              | Config directory (`config.json` lives here) |
| `gokapi_sys_user`     | `gokapi`                              | System user                                 |
| `gokapi_sys_group`    | `gokapi`                              | System group                                |
| `gokapi_port`         | `53842`                               | Local port for Caddy reverse proxy          |
| `gokapi_domain`       | `{{ gokapi_subdomain }}.{{ domain }}` | Public hostname                             |
| `gokapi_version`      | `2.2.4`                               | Pinned upstream release                     |

## First-deploy setup wizard

Gokapi has no environment-variable bootstrap for admin credentials. On the first deploy, after Ansible finishes:

1. Visit `https://{{ gokapi_domain }}/setup` from a browser.
2. Enter the admin username and password matching `gokapi_admin_user` and `gokapi_admin_password` from `config.toml`.
3. Pick SQLite (local disk). Save and the wizard exits.

The `/setup` endpoint is unauthenticated until completion. Caddy will already be proxying the public hostname when Ansible finishes, so do this immediately after the deploy.

Subsequent deploys are idempotent: `config.json` exists, so Gokapi skips the wizard.

To rotate the admin password later, run `gokapi --deployment-password <new>` as root on the host (binary at `{{ gokapi_install_path }}/gokapi`); it sets the password on the existing super-admin and exits.

## Management

```bash
sudo systemctl status gokapi
sudo journalctl -u gokapi -f
sudo systemctl restart gokapi
```

## Backup

`/var/lib/gokapi` contains the SQLite DB and uploaded files. The Playbook Meta's Backup Recipe captures it.

## Dependencies

- Caddy reverse proxy (substrate)
- Cloudflare DNS API token (`cloudflare_dns_api_token`)
- UFW must allow 443/tcp on the public interface (default in `ufw` role)
