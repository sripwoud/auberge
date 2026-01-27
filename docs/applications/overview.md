# Applications Overview

Auberge deploys a curated stack of self-hosted FOSS applications. All services run natively via systemd without Docker overhead.

## Infrastructure

| Application                            | Description                        |
| -------------------------------------- | ---------------------------------- |
| [Caddy](infrastructure/caddy.md)       | Reverse proxy with automatic HTTPS |
| [fail2ban](infrastructure/fail2ban.md) | Intrusion prevention system        |
| [UFW](infrastructure/ufw.md)           | Uncomplicated firewall             |

## Networking

| Application                          | Description                          |
| ------------------------------------ | ------------------------------------ |
| [Blocky](networking/blocky.md)       | DNS server with ad/tracking blocking |
| [WireGuard](networking/wireguard.md) | Fast, modern VPN                     |
| [Tailscale](networking/tailscale.md) | Mesh VPN for secure remote access    |

## Apps

| Application                    | Description                       |
| ------------------------------ | --------------------------------- |
| [Radicale](apps/radicale.md)   | Lightweight CalDAV/CardDAV server |
| [FreshRSS](apps/freshrss.md)   | RSS feed aggregator               |
| [Navidrome](apps/navidrome.md) | Music streaming server            |
| [Calibre](apps/calibre.md)     | Ebook library management          |
| [WebDAV](apps/webdav.md)       | File sharing and synchronization  |
| [YOURLS](apps/yourls.md)       | URL shortener                     |

## Deployment

All applications are deployed via Ansible playbooks. See [Running Playbooks](../deployment/running-playbooks.md) for details.

```bash
# Deploy all apps
auberge ansible run --playbook playbooks/apps.yml

# Deploy specific app
auberge ansible run --tags radicale
```

## Backup Support

All applications support backup and restore. See [Backup & Restore](../backup-restore/overview.md) for details.
