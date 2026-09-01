# Environment Variables

All values live in `~/.config/auberge/config.toml`. Manage with `auberge config set <key> <value>`.

| Variable                   | Required | Description                                                                                                                                                  |
| -------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `admin_user_name`          | Yes      | Server admin username                                                                                                                                        |
| `admin_user_email`         | Yes      | Server admin email                                                                                                                                           |
| `domain`                   | Yes      | Primary domain (e.g. `example.com`)                                                                                                                          |
| `ssh_port`                 | Yes      | SSH port for managed hosts                                                                                                                                   |
| `cloudflare_dns_api_token` | Yes      | Cloudflare token with DNS Edit + Zone Read; required for DNS commands and ACME challenges                                                                    |
| `tailscale_authkey`        | Optional | Pre-auth key for the tailnet. [Minted per run by the CLI](applications/networking/headscale.md); set it only to bootstrap the host that will serve Headscale |
| `tailscale_api_key`        | Optional | Enables automatic Blocky DNS configuration via Tailscale API                                                                                                 |
| `tailscale_login_server`   | Optional | Self-hosted Headscale URL (e.g. `https://hs.example.com`); omit to use Tailscale SaaS                                                                        |
| `restic_repository`        | Optional | Restic destination (e.g. `rclone:offsite:auberge-backup`); required for `backup push`                                                                        |
| `restic_password`          | Optional | Restic encryption passphrase — NOT your cloud storage password; required for `backup push`                                                                   |
| `actual_subdomain`         | Optional | Subdomain for Actual Budget (default: `actual`; always tailnet-only)                                                                                         |
| `baikal_subdomain`         | Optional | Subdomain for Baïkal                                                                                                                                         |
| `bichon_subdomain`         | Optional | Subdomain for Bichon                                                                                                                                         |
| `bichon_tailscale_ip`      | Optional | Tailnet address Blocky publishes for Bichon (see below)                                                                                                      |
| `blocky_subdomain`         | Optional | Subdomain for Blocky                                                                                                                                         |
| `freshrss_subdomain`       | Optional | Subdomain for FreshRSS                                                                                                                                       |
| `headscale_subdomain`      | Optional | Subdomain for Headscale; unset means Headscale is not deployed                                                                                               |
| `navidrome_subdomain`      | Optional | Subdomain for Navidrome                                                                                                                                      |
| `paperless_subdomain`      | Optional | Subdomain for Paperless                                                                                                                                      |
| `paperless_tailscale_ip`   | Optional | Tailnet address Blocky publishes for Paperless (see below)                                                                                                   |
| `gokapi_subdomain`         | Optional | Subdomain for Gokapi (default: `share`)                                                                                                                      |
| `yourls_subdomain`         | Optional | Subdomain for YOURLS                                                                                                                                         |

?> **`<app>_tailscale_ip` for a tailnet-only app** — one whose playbook meta declares `tailnet_only: true` — is the address Blocky's `customDNS` answers with. Leave it unset and Blocky answers with its own host's tailnet address, which is right whenever the app runs on the host running Blocky ([ADR-0059](https://github.com/sripwoud/auberge/blob/master/meta/adr/0059-a-tailnet-only-apps-address-is-per-app.md)). Set it only for an app on another host, and set it at the **top level** of `config.toml`: Blocky builds the whole fleet's map in one run against its own host, so a [`[hosts.<name>]`](configuration/host-scoped-config.md) answer scoped to the app's own host never reaches that run. Setting it also arms `auberge deploy`'s [tailnet DNS check](cli-reference/deploy.md#dns-verification) for that app, which queries Blocky's host — so that host needs a cached `tailscale_ip` too.

?> **`<app>_tailscale_ip` for a public app** is the address `dns set-all` writes as the A record instead of the public server IP. Public internet cannot route CGNAT addresses (`100.64.0.0/10`), so no firewall rules are needed; `dns migrate` skips records already holding one.

?> All values support `!` command syntax to fetch secrets from a password manager: `auberge config set restic_password '!pass show auberge/restic'`. See [Secrets Management](configuration/secrets.md#password-commands).
