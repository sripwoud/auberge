<p align="center">
  <h1 align="center"><a href="https://auberge.sripwoud.xyz">Auberge</a></h1>
</p>
<p align="center"
  <a href="https://crates.io/crates/auberge">
    <img src="https://img.shields.io/crates/v/auberge" alt="Crates.io">
  </a>
</p>

> Ansible-powered VPS management without Docker bloat

Auberge is a CLI tool for managing self-hosted infrastructure using Ansible automation. Deploy a full FOSS stack on a minimal VPS with no container overhead.

## Features

- **Minimal footprint**: Runs on a 2GB RAM VPS (4GB recommended with Booklore)
- **No Docker bloat**: Native systemd services
- **Automated deployment**: Ansible playbooks handle everything
- **Built-in backups**: Full backup and restore with cross-host migration
- **DNS management**: Cloudflare integration
- **SSH security**: Multi-tier key management

## Quick Start

Install Auberge from a [pre-compiled binary](https://github.com/sripwoud/auberge/releases/latest) or via cargo:

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

- **Infrastructure**: [Caddy](https://caddyserver.com) (reverse proxy), [fail2ban](https://github.com/fail2ban/fail2ban) (intrusion prevention), [UFW](https://launchpad.net/ufw) (firewall)
- **Networking**: [Blocky](https://0xerr0r.github.io/blocky) (DNS + ad-blocking), [Headscale](https://headscale.net) (coordination server), [WireGuard](https://wireguard.com), [Tailscale](https://tailscale.com)
- **Apps**: [Baikal](https://sabre.io/baikal) (calendar/contacts), [Bichon](https://github.com/rustmailer/bichon) (email archiving), [Booklore](https://booklore.org) (digital library), [Colporteur](https://github.com/sripwoud/colporteur) (newsletter-to-feed), [FreshRSS](https://freshrss.org) (RSS reader), [Navidrome](https://navidrome.org) (music), [Paperless-ngx](https://docs.paperless-ngx.com) (document management), [Syncthing](https://syncthing.net) (file sync), [WebDAV](https://github.com/hacdias/webdav) (file sharing), [YOURLS](https://yourls.org) (URL shortener)

## Documentation

Full documentation available at [auberge.sripwoud.xyz](https://auberge.sripwoud.xyz):

- [Installation](https://auberge.sripwoud.xyz/#/getting-started/installation) - Detailed setup guide
- [First Deployment](https://auberge.sripwoud.xyz/#/getting-started/first-deployment) - Step-by-step walkthrough
- [CLI Reference](https://auberge.sripwoud.xyz/#/cli-reference/auberge) - All commands documented
- [Backup & Restore](https://auberge.sripwoud.xyz/#/backup-restore/overview) - Data protection and migration
- [Alternatives Comparison](https://auberge.sripwoud.xyz/#/about/alternatives) - How Auberge compares to other platforms

## Requirements

- Linux or macOS (Windows is not supported — Ansible requires a Unix-based control node)
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
