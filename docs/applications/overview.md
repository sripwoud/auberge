# Applications Overview

Auberge deploys a curated stack of self-hosted FOSS applications. All services run natively via systemd without Docker overhead.

## Infrastructure

| Application                            | Description                        |
| -------------------------------------- | ---------------------------------- |
| [Caddy](infrastructure/caddy.md)       | Reverse proxy with automatic HTTPS |
| [Cockpit](infrastructure/cockpit.md)   | Web-based server administration    |
| [fail2ban](infrastructure/fail2ban.md) | Intrusion prevention system        |
| [UFW](infrastructure/ufw.md)           | Uncomplicated firewall             |

## Networking

| Application                          | Description                          |
| ------------------------------------ | ------------------------------------ |
| [Blocky](networking/blocky.md)       | DNS server with ad/tracking blocking |
| [Headscale](networking/headscale.md) | Self-hosted Tailscale control server |
| [WireGuard](networking/wireguard.md) | Fast, modern VPN                     |
| [Tailscale](networking/tailscale.md) | Mesh VPN for secure remote access    |

## Apps

| Application                        | Description                             |
| ---------------------------------- | --------------------------------------- |
| [Baikal](apps/baikal.md)           | CalDAV/CardDAV server                   |
| [Bichon](apps/bichon.md)           | Email archiving and search              |
| [Grimmory](apps/grimmory.md)       | Multi-user digital library              |
| [Calibre](apps/calibre.md)         | Ebook library (alternative to Grimmory) |
| [Colporteur](apps/colporteur.md)   | Newsletter-to-feed converter            |
| [FreshRSS](apps/freshrss.md)       | RSS feed aggregator                     |
| [Navidrome](apps/navidrome.md)     | Music streaming server                  |
| [Paperless-ngx](apps/paperless.md) | Document management system              |
| [Syncthing](apps/syncthing.md)     | Continuous file synchronization         |
| [WebDAV](apps/webdav.md)           | File sharing and synchronization        |
| [YOURLS](apps/yourls.md)           | URL shortener                           |

## Deployment

All applications are deployed via Ansible playbooks. See [Running Playbooks](../deployment/running-playbooks.md) for details.

```bash
# Deploy all apps
auberge deploy --all

# Deploy specific app
auberge deploy baikal
```

## Backup Support

All applications support backup and restore. See [Backup & Restore](../backup-restore/overview.md) for details.
