# Quick Start

Get Auberge running in 5 minutes.

## 1. Install

```bash
cargo install auberge
```

## 2. Add Your VPS

```bash
auberge host add my-vps 203.0.113.10
```

Follow the prompts to enter SSH user and port (defaults to current user, port 22).

## 3. Deploy

```bash
auberge ansible run
```

Select:

1. Your VPS host
2. The "auberge" playbook (complete stack)

Sit back while Auberge configures everything.

## What Just Happened?

Auberge just:

- Secured your VPS (SSH hardening, firewall, fail2ban)
- Installed Caddy reverse proxy with automatic HTTPS
- Deployed all applications with systemd services
- Configured DNS and networking

## Access Your Services

Services are available at subdomains of your primary domain:

- `https://cal.yourdomain.com` - Baikal (calendar/contacts)
- `https://rss.yourdomain.com` - FreshRSS
- `https://musique.yourdomain.com` - Navidrome
- `https://lire.yourdomain.com` - Calibre
- `https://webdav.yourdomain.com` - WebDAV
- `https://url.yourdomain.com` - YOURLS

## Next Steps

- [First Deployment](first-deployment.md) - Detailed walkthrough
- [Backup & Restore](../backup-restore/overview.md) - Protect your data
- [CLI Reference](../cli-reference/auberge.md) - All commands
