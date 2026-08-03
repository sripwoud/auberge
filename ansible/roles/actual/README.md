# Actual Ansible Role

Deploys the [Actual Budget](https://actualbudget.org) sync server (`@actual-app/sync-server`) as a bare-metal npm install behind Caddy, reachable only over the tailnet.

## Features

- Sync relay + encrypted blob store for Actual clients (each client keeps a full local copy of the budget)
- Bank sync via Enable Banking (credentials entered once in the web UI, stored server-side; see ADR-0016)
- Node.js from NodeSource (Debian trixie ships Node 20; sync-server requires `>=22`)
- Loopback-only Node process; Caddy binds the Tailscale IP with DNS-01 TLS

## Variables

| Variable             | Default                               | Description                                  |
| -------------------- | ------------------------------------- | -------------------------------------------- |
| `actual_sys_user`    | `actual`                              | System user                                  |
| `actual_sys_group`   | `actual`                              | System group                                 |
| `actual_port`        | `5006`                                | Local port for Caddy reverse proxy           |
| `actual_subdomain`   | `actual`                              | Subdomain (operator override in config.toml) |
| `actual_domain`      | `{{ actual_subdomain }}.{{ domain }}` | Tailnet hostname                             |
| `actual_install_dir` | `/opt/actual`                         | npm install prefix (root-owned)              |
| `actual_data_dir`    | `/var/lib/actual`                     | `server-files/` + `user-files/`              |
| `actual_version`     | `26.8.0`                              | Pinned `@actual-app/sync-server` release     |
| `actual_node_major`  | `22`                                  | NodeSource major (minors float via apt)      |

## First deploy

1. Open `https://<actual_domain>` from a tailnet device and set the server password (Actual's own onboarding; no headless bootstrap needed — the first visitor claims the server, and the vhost is tailnet-only).
2. Create or import a budget; add the same server URL + password on other devices to sync.
3. Bank sync: More → Bank Sync → Set up Enable Banking (Application ID + credential file from enablebanking.com/cp/applications).

## Data & backup

Everything lives in `/var/lib/actual`: `server-files/account.sqlite` (server accounts, file registry) and `user-files/` (budget blobs). The Backup Recipe in `actual.meta.yml` stops the unit and rsyncs the whole directory. Losing it does not lose budgets — clients hold full copies and can re-upload — but it does lose the server password and bank-sync credentials.

## Upload limits

Actual enforces its own limits (20 MB files, 50 MB encrypted sync blobs). The Caddy vhost caps request bodies at 100M — above anything Actual accepts, so the cap is never the binding constraint; Caddy is otherwise unlimited by default.
