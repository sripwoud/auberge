# Auberge

Selfware for managing my self-hosted FOSS stack, built around Ansible automation. No Docker bloat, runs lean on a 1€/month 1GB VPS ([IONOS Linux VPS XS](https://www.ionos.de/server/vps)).

## Installation

```bash
cargo install auberge
```

## Quick Start

Add your VPS as a host:

```bash
auberge host add my-vps 194.164.53.11
# Prompts for SSH user and port (defaults to current user, port 22)
```

Deploy the full stack:

```bash
auberge ansible run
# 1. Select your VPS host
# 2. Select "auberge" playbook for the complete stack
# 3. Sit back while it configures everything
```

Available commands:

```bash
# Host management
auberge host add <name> <ip>   # Add a VPS host
auberge host list              # List all hosts
auberge host remove <name>     # Remove a host

# Deployment
auberge ansible run            # Interactive playbook execution
auberge ansible bootstrap      # Initial VPS setup (first time only)
auberge ansible check          # Dry-run to preview changes

# Other
auberge dns <subcommand>       # DNS management via Cloudflare
auberge ssh keygen             # Generate SSH keys for hosts
```

## Stack

### Infrastructure

| Name                                             | Description                        |
| ------------------------------------------------ | ---------------------------------- |
| [Caddy](https://caddyserver.com)                 | Reverse proxy with automatic HTTPS |
| [fail2ban](https://github.com/fail2ban/fail2ban) | Intrusion prevention system        |
| [UFW](https://launchpad.net/ufw)                 | Uncomplicated firewall             |

### Apps

| Category      | Name                                        | Description                          |
| ------------- | ------------------------------------------- | ------------------------------------ |
| Ad-blocker    | [Blocky](https://0xerr0r.github.io/blocky)  | DNS server with ad/tracking blocking |
| VPN           | [WireGuard](https://wireguard.com)          | Fast, modern VPN                     |
| VPN           | [Tailscale](https://tailscale.com)          | Mesh VPN for secure remote access    |
| Calendar      | [Radicale](https://radicale.org)            | Lightweight CalDAV/CardDAV server    |
| File sharing  | [WebDAV](https://github.com/hacdias/webdav) | File sharing and synchronization     |
| Books         | [Calibre](https://calibre-ebook.com)        | Ebook library management             |
| Music         | [Navidrome](https://navidrome.org)          | Music streaming server               |
| News          | [FreshRSS](https://freshrss.org)            | RSS feed aggregator                  |
| URL shortener | [YOURLS](https://yourls.org)                | URL shortener                        |

## Develop

See [develop.md](develop.md) for local development setup.

## Playbooks

Playbooks are organized in layers:

| Playbook         | Description                                                    |
| ---------------- | -------------------------------------------------------------- |
| `bootstrap`      | Initial VPS setup - creates users and secures SSH              |
| `hardening`      | Security hardening - firewall, intrusion prevention, kernel    |
| `infrastructure` | Core infrastructure - package management, shell, reverse proxy |
| `apps`           | Self-hosted applications layer                                 |
| `auberge` ⭐     | **Master playbook** - runs all layers (bootstrap → apps)       |

Run individual layers with tags:

```bash
auberge ansible run --tags hardening  # Security layer only
auberge ansible run --tags caddy      # Just the reverse proxy
```
