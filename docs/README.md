# Auberge

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

## Next Steps

- [Installation](getting-started/installation.md) - Detailed setup guide
- [First Deployment](getting-started/first-deployment.md) - Step-by-step walkthrough
- [CLI Reference](cli-reference/auberge.md) - All commands documented
- [Backup & Restore](backup-restore/overview.md) - Data protection and migration

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

- [GitHub Repository](https://github.com/sripwoud/auberge)
- [Report Issues](https://github.com/sripwoud/auberge/issues)
- [Contribute](development/contributing.md)
