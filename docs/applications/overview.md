# Applications Overview

Auberge deploys a curated stack of self-hosted FOSS applications. Services run natively via systemd; containers are per-app exceptions granted only when upstream supports nothing else (currently Immich).

## Infrastructure

| Application                                         | Description                        |
| --------------------------------------------------- | ---------------------------------- |
| [Caddy](applications/infrastructure/caddy.md)       | Reverse proxy with automatic HTTPS |
| [Cockpit](applications/infrastructure/cockpit.md)   | Web-based server administration    |
| [fail2ban](applications/infrastructure/fail2ban.md) | Intrusion prevention system        |
| [UFW](applications/infrastructure/ufw.md)           | Uncomplicated firewall             |

## Networking

| Application                                       | Description                          |
| ------------------------------------------------- | ------------------------------------ |
| [Blocky](applications/networking/blocky.md)       | DNS server with ad/tracking blocking |
| [Headscale](applications/networking/headscale.md) | Self-hosted Tailscale control server |
| [WireGuard](applications/networking/wireguard.md) | Fast, modern VPN                     |
| [Tailscale](applications/networking/tailscale.md) | Mesh VPN for secure remote access    |

## Apps

| Application                                     | Description                             |
| ----------------------------------------------- | --------------------------------------- |
| [Actual Budget](applications/apps/actual.md)    | Budgeting with EU bank sync             |
| [Baikal](applications/apps/baikal.md)           | CalDAV/CardDAV server                   |
| [Bichon](applications/apps/bichon.md)           | Email archiving and search              |
| [Grimmory](applications/apps/grimmory.md)       | Multi-user digital library              |
| [Calibre](applications/apps/calibre.md)         | Ebook library (alternative to Grimmory) |
| [Colporteur](applications/apps/colporteur.md)   | Newsletter-to-feed converter            |
| [FreshRSS](applications/apps/freshrss.md)       | RSS feed aggregator                     |
| [Immich](applications/apps/immich.md)           | Photo and video management              |
| [Navidrome](applications/apps/navidrome.md)     | Music streaming server                  |
| [Paperless-ngx](applications/apps/paperless.md) | Document management system              |
| [Radio](applications/apps/radio.md)             | Password-gated music streams            |
| [Syncthing](applications/apps/syncthing.md)     | Continuous file synchronization         |
| [Gokapi](applications/apps/gokapi.md)           | Expiring-link file sharing              |
| [YOURLS](applications/apps/yourls.md)           | URL shortener                           |

## Notifications

| Application                           | Description                               |
| ------------------------------------- | ----------------------------------------- |
| [TGTG Bot](applications/apps/tgtg.md) | Too Good To Go availability notifications |

## AI

| Application                                 | Description                          |
| ------------------------------------------- | ------------------------------------ |
| [Hermes Agent](applications/apps/hermes.md) | Self-improving personal AI assistant |

## Deployment

All applications are deployed via Ansible playbooks. See [Running Playbooks](cli-reference/ansible/run.md) for details.

```bash
# Deploy all apps
auberge deploy --all

# Deploy specific app
auberge deploy baikal
```

## Backup Support

All applications support backup and restore. See [Backup & Restore](backup-restore/overview.md) for details.
