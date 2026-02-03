# [Auberge](https://auberge.sripwoud.xyz)

> Ansible-powered VPS management without Docker bloat

Auberge is a CLI tool for managing self-hosted infrastructure using Ansible automation. Deploy a full FOSS stack on a minimal VPS with no container overhead.

## Features

- **Minimal footprint**: Runs on 1GB RAM VPS
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
auberge host add my-vps 194.164.53.11
```

Deploy the full stack:

```bash
auberge ansible run
```

That's it! Auberge will configure your VPS with all infrastructure and applications.

## What You Get

After deployment, your VPS will be running:

- **Infrastructure**: Caddy (reverse proxy), fail2ban (intrusion prevention), UFW (firewall)
- **Networking**: Blocky (DNS + ad-blocking), WireGuard, Tailscale
- **Apps**: Baikal (calendar/contacts), FreshRSS (RSS reader), Navidrome (music), Calibre (books), WebDAV (file sharing), YOURLS (URL shortener)

## Documentation

Full documentation available at [auberge.sripwoud.xyz](https://auberge.sripwoud.xyz):

- [Installation](https://auberge.sripwoud.xyz/#/getting-started/installation) - Detailed setup guide
- [First Deployment](https://auberge.sripwoud.xyz/#/getting-started/first-deployment) - Step-by-step walkthrough
- [CLI Reference](https://auberge.sripwoud.xyz/#/cli-reference/auberge) - All commands documented
- [Backup & Restore](https://auberge.sripwoud.xyz/#/backup-restore/overview) - Data protection and migration

## Requirements

- Rust/Cargo for installation
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

- [Documentation](https://auberge.sripwoud.xyz)
- [Report Issues](https://github.com/sripwoud/auberge/issues)
- [Contributing](https://auberge.sripwoud.xyz/#/development/contributing)
