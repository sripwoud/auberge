# Introduction

Auberge (French for _inn_) shelters your self-hosted services. It's a Rust CLI that wraps Ansible to deploy a complete FOSS stack on a minimal VPS — without containers.

## How it works

```
auberge CLI ─→ Ansible playbooks ─→ systemd services on the VPS
```

1. Define hosts in `~/.config/auberge/hosts.toml` (or via `auberge host add`).
2. Set required values in `config.toml` (domain, secrets, …).
3. `auberge deploy <app>` runs the relevant playbooks via SSH.

Backups, DNS sync, and offsite restic snapshots are first-class CLI verbs.

## What you get

**Infrastructure**: [Caddy](https://caddyserver.com), [fail2ban](https://github.com/fail2ban/fail2ban), [UFW](https://launchpad.net/ufw)

**Networking**: [Blocky](https://0xerr0r.github.io/blocky), [Headscale](https://headscale.net), [WireGuard](https://wireguard.com), [Tailscale](https://tailscale.com)

**Apps**: [Baikal](https://sabre.io/baikal), [Bichon](https://github.com/rustmailer/bichon), [Grimmory](https://grimmory.org), [Colporteur](https://github.com/sripwoud/colporteur), [FreshRSS](https://freshrss.org), [Gokapi](https://github.com/Forceu/Gokapi), [Navidrome](https://navidrome.org), [Paperless-ngx](https://docs.paperless-ngx.com), [Syncthing](https://syncthing.net), [YOURLS](https://yourls.org)

## Requirements

- Linux or macOS workstation (Windows: use WSL2)
- VPS with root/sudo access — 2 GB RAM minimum, 4 GB recommended with Grimmory
- (Optional) Cloudflare account for managed DNS

## Philosophy

_Selfware_: direct control, no abstraction layers, no container runtime, transparent operations.

Continue: [Quick Start](getting-started/quick-start.md) · [Installation](getting-started/installation.md) · [First Deployment](getting-started/first-deployment.md)
