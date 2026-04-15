# Auberge

> Ansible-powered VPS management without Docker bloat

Auberge is a CLI tool for managing self-hosted infrastructure using Ansible automation. Deploy a full FOSS stack on a minimal VPS with no container overhead.

## Features

- **Minimal footprint**: Runs on a 2GB RAM VPS (4GB recommended with Grimmory)
- **No Docker bloat**: Native systemd services
- **Automated deployment**: Ansible playbooks handle everything
- **Built-in backups**: Full backup and restore with cross-host migration
- **DNS management**: Cloudflare integration
- **SSH security**: Multi-tier key management

## Quick Start

Install Auberge:

```bash
cargo install auberge
```

Add your VPS:

```bash
auberge host add my-vps 203.0.113.10
```

Deploy the full stack:

```bash
auberge deploy --all
```

That's it! Auberge will configure your VPS with all infrastructure and applications.

## What You Get

After deployment, your VPS will be running:

- **Infrastructure**: [Caddy](applications/infrastructure/caddy.md) (reverse proxy), [fail2ban](applications/infrastructure/fail2ban.md) (intrusion prevention), [UFW](applications/infrastructure/ufw.md) (firewall)
- **Networking**: [Blocky](applications/networking/blocky.md) (DNS + ad-blocking), [WireGuard](applications/networking/wireguard.md), [Tailscale](applications/networking/tailscale.md)
- **Apps**: [Baikal](applications/apps/baikal.md) (calendar/contacts), [Colporteur](applications/apps/colporteur.md) (newsletter-to-feed), [FreshRSS](applications/apps/freshrss.md) (RSS reader), [Navidrome](applications/apps/navidrome.md) (music), [Grimmory](applications/apps/grimmory.md) (books), [Paperless-ngx](applications/apps/paperless.md) (documents), [WebDAV](applications/apps/webdav.md) (file sharing), [YOURLS](applications/apps/yourls.md) (URL shortener)
- **AI Agent**: [Hermes Agent](applications/apps/hermes.md) (self-improving personal AI assistant via Telegram)

## Next Steps

- [Installation](getting-started/installation.md) - Detailed setup guide
- [First Deployment](getting-started/first-deployment.md) - Step-by-step walkthrough
- [CLI Reference](cli-reference/auberge.md) - All commands documented
- [Backup & Restore](backup-restore/overview.md) - Data protection and migration

## Requirements

- A VPS with root/sudo access
- SSH connectivity to your VPS
- (Optional) Cloudflare account for DNS management

## Philosophy

Auberge is selfware - software built to manage your self-hosted infrastructure:

- Lean and efficient
- No unnecessary abstractions
- Direct control over your services
- Transparent operations

## Community

- [GitHub Repository](https://github.com/sripwoud/auberge)
- [Report Issues](https://github.com/sripwoud/auberge/issues)
- [Contribute](development/contributing.md)
