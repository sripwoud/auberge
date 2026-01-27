# Introduction

Auberge is a CLI tool for managing self-hosted infrastructure using Ansible automation. It deploys a complete FOSS stack on minimal VPS hardware without Docker overhead.

## What is Auberge?

Auberge (French for "inn") provides shelter for your self-hosted services. It's selfware - software built to manage your self-hosted infrastructure with:

- **Minimal footprint**: Runs on 1GB RAM VPS (1€/month IONOS Linux VPS XS)
- **No Docker bloat**: Native systemd services
- **Automated deployment**: Ansible playbooks handle everything
- **Built-in backups**: Full backup and restore with cross-host migration
- **DNS management**: Cloudflare integration

## Philosophy

- Lean and efficient
- No unnecessary abstractions
- Direct control over your services
- Transparent operations

## What You Get

After deployment, your VPS will be running:

**Infrastructure**:

- Caddy (reverse proxy with automatic HTTPS)
- fail2ban (intrusion prevention)
- UFW (firewall)

**Networking**:

- Blocky (DNS + ad-blocking)
- WireGuard
- Tailscale

**Applications**:

- Radicale (calendar/contacts)
- FreshRSS (RSS reader)
- Navidrome (music streaming)
- Calibre (ebook library)
- WebDAV (file sharing)
- YOURLS (URL shortener)

## How It Works

Auberge is a Rust CLI that wraps Ansible playbooks:

1. You define hosts in `~/.config/auberge/hosts.toml`
2. Auberge runs Ansible playbooks on your VPS
3. Services are installed and configured via systemd
4. Backups can be created and restored anytime

## Requirements

- Rust/Cargo for installation
- A VPS with root/sudo access
- SSH connectivity to your VPS
- (Optional) Cloudflare account for DNS management

## Next Steps

- [Quick Start](quick-start.md) - 5-minute setup
- [Installation](installation.md) - Detailed installation guide
- [First Deployment](first-deployment.md) - Step-by-step walkthrough
