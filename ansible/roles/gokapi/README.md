# Gokapi Ansible Role

Deploys [Gokapi](https://github.com/Forceu/Gokapi) — a Go single-binary, self-hosted Firefox Send alternative — as a systemd service behind Caddy.

## Features

- Link-based file sharing with expiry by download count or time
- File Requests: per-URL upload links visible only to the creator
- Optional per-share password
- SQLite backend (default) or S3
- Optional OIDC integration

## Variables

| Variable                  | Default                                 | Description                                            |
| ------------------------- | --------------------------------------- | ------------------------------------------------------ |
| `gokapi_install_path`     | `/opt/gokapi`                           | Binary install directory                               |
| `gokapi_data_dir`         | `/var/lib/gokapi`                       | Persistent data + DB directory                         |
| `gokapi_config_dir`       | `/var/lib/gokapi/config`                | Config directory (`config.json` lives here)            |
| `gokapi_sys_user`         | `gokapi`                                | System user                                            |
| `gokapi_sys_group`        | `gokapi`                                | System group                                           |
| `gokapi_port`             | `53842`                                 | Local port for Caddy reverse proxy                     |
| `gokapi_domain`           | `{{ gokapi_subdomain }}.{{ domain }}`   | Public hostname                                        |
| `gokapi_version`          | `2.2.4`                                 | Pinned upstream release                                |
| `gokapi_bootstrap_marker` | `{{ gokapi_data_dir }}/.bootstrap_done` | Sentinel file gating the one-shot superadmin bootstrap |

`gokapi_admin_user` and `gokapi_admin_password` are required keys from the operator's `config.toml` (declared in the Playbook Meta).

## Headless first-deploy

The role bootstraps Gokapi without the interactive setup wizard. See ADR-0009 for the rationale and security analysis.

1. `config.json` is templated at `{{ gokapi_config_dir }}/config.json` with `force: false`. This means it is written exactly once, then never overwritten — Gokapi's admin UI persists its own changes to the same file via `configuration.save()`, and those operator edits must survive subsequent deploys.
2. Before the systemd service starts, `gokapi --deployment-password <pw>` runs once as the `gokapi` user. Gokapi's `EditSuperAdmin` creates the superadmin in its database (Username from the templated `config.json`, password hashed by Gokapi using its own KDF), then exits 0.
3. The bootstrap marker (`{{ gokapi_bootstrap_marker }}`) is touched. Subsequent deploys see the marker and skip the one-shot.
4. The systemd service is enabled and started. `RunIfFirstStart` sees `config.json` exists and never opens the setup webserver; `checkIfUserExists` finds the superadmin and passes.
5. Caddy site and Cloudflare A record deploy unconditionally in the same `auberge deploy` invocation.

## Rotating the admin password

Auberge does not propagate password changes automatically (the marker file blocks the one-shot from re-running). To rotate:

```bash
ssh <host> sudo rm /var/lib/gokapi/.bootstrap_done
auberge deploy gokapi
```

The next deploy sees the marker missing, re-runs `--deployment-password` with the new value from `config.toml`. `EditSuperAdmin` updates the existing superadmin's password and exits 0.

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
