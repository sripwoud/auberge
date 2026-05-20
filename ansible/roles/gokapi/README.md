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

Gokapi has no environment-variable bootstrap for admin credentials, and its `/setup` endpoint is unauthenticated until completion — anyone reaching the listener first becomes admin. The role mitigates this by **gating public exposure** on `config.json` existing:

- **First deploy**: gokapi binary + systemd unit installed only. Caddy site and Cloudflare DNS record are deliberately NOT created. Gokapi listens on `:53842` but UFW blocks that port from the public internet, so the only path in is an SSH tunnel from the operator's laptop.
- Operator runs:
  ```bash
  ssh -L 53842:127.0.0.1:53842 <ansible-user>@<host>
  # in a browser: http://localhost:53842/setup
  ```
  Enter the admin username and password matching `gokapi_admin_user` / `gokapi_admin_password` from `config.toml`, pick SQLite, save.
- **Second deploy**: `config.json` now exists, so the role deploys the Caddy site and creates the Cloudflare A record. `https://{{ gokapi_domain }}` is now live.

Subsequent deploys are fully idempotent — no manual step.

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
