# Environment Variables

All values live in `~/.config/auberge/config.toml`. Manage with `auberge config set <key> <value>`.

| Variable                   | Required | Description                                                                                |
| -------------------------- | -------- | ------------------------------------------------------------------------------------------ |
| `admin_user_name`          | Yes      | Server admin username                                                                      |
| `admin_user_email`         | Yes      | Server admin email                                                                         |
| `domain`                   | Yes      | Primary domain (e.g. `example.com`)                                                        |
| `ssh_port`                 | Yes      | SSH port for managed hosts                                                                 |
| `cloudflare_dns_api_token` | Yes      | Cloudflare token with DNS Edit + Zone Read; required for DNS commands and ACME challenges  |
| `tailscale_authkey`        | Yes      | Tailscale (or Headscale) pre-auth key for VPN mesh                                         |
| `tailscale_api_key`        | Optional | Enables automatic Blocky DNS configuration via Tailscale API                               |
| `tailscale_login_server`   | Optional | Self-hosted Headscale URL (e.g. `https://hs.example.com`); omit to use Tailscale SaaS      |
| `restic_repository`        | Optional | Restic destination (e.g. `rclone:filen:auberge-backup`); required for `backup push`        |
| `restic_password`          | Optional | Restic encryption passphrase — NOT your cloud storage password; required for `backup push` |
| `baikal_subdomain`         | Optional | Subdomain for Baïkal                                                                       |
| `bichon_subdomain`         | Optional | Subdomain for Bichon                                                                       |
| `bichon_tailscale_ip`      | Optional | Tailscale IP; makes subdomain tailnet-only (see below)                                     |
| `blocky_subdomain`         | Optional | Subdomain for Blocky                                                                       |
| `freshrss_subdomain`       | Optional | Subdomain for FreshRSS                                                                     |
| `headscale_subdomain`      | Optional | Subdomain for Headscale                                                                    |
| `navidrome_subdomain`      | Optional | Subdomain for Navidrome                                                                    |
| `paperless_subdomain`      | Optional | Subdomain for Paperless                                                                    |
| `paperless_tailscale_ip`   | Optional | Tailscale IP; makes subdomain tailnet-only                                                 |
| `gokapi_subdomain`         | Optional | Subdomain for Gokapi (default: `share`)                                                    |
| `yourls_subdomain`         | Optional | Subdomain for YOURLS                                                                       |

?> **Tailnet-only subdomains**: setting `<app>_tailscale_ip` causes `dns set-all` to point that subdomain's A record at the Tailscale CGNAT IP (`100.64.0.0/10`) instead of the public server IP. Public internet cannot route CGNAT addresses, so no firewall rules are needed. `dns migrate` skips records whose current IP is in the CGNAT range.

?> All values support `!` command syntax to fetch secrets from a password manager: `auberge config set restic_password '!pass show auberge/restic'`. See [Secrets Management](configuration/secrets.md#password-commands).
