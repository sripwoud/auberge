# Introduction

Auberge is a CLI tool for managing self-hosted infrastructure using Ansible automation. It deploys a complete FOSS stack on minimal VPS hardware without Docker overhead.

## What is Auberge?

Auberge (French for "inn") provides shelter for your self-hosted services. It's selfware - software built to manage your self-hosted infrastructure with:

- **Minimal footprint**: Runs on a 2GB RAM VPS (4GB recommended with Grimmory)
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

- [Caddy](https://caddyserver.com) (reverse proxy with automatic HTTPS)
- [fail2ban](https://github.com/fail2ban/fail2ban) (intrusion prevention)
- [UFW](https://launchpad.net/ufw) (firewall)

**Networking**:

- [Blocky](https://0xerr0r.github.io/blocky) (DNS + ad-blocking)
- [Headscale](https://headscale.net) (coordination server)
- [WireGuard](https://wireguard.com)
- [Tailscale](https://tailscale.com)

**Applications**:

- [Baikal](https://sabre.io/baikal) (calendar/contacts)
- [Bichon](https://github.com/rustmailer/bichon) (email archiving)
- [Grimmory](https://grimmory.org) (digital library)
- [Colporteur](https://github.com/sripwoud/colporteur) (newsletter-to-feed)
- [FreshRSS](https://freshrss.org) (RSS reader)
- [Navidrome](https://navidrome.org) (music streaming)
- [Paperless-ngx](https://docs.paperless-ngx.com) (document management)
- [Syncthing](https://syncthing.net) (file sync)
- [WebDAV](https://github.com/hacdias/webdav) (file sharing)
- [YOURLS](https://yourls.org) (URL shortener)

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
