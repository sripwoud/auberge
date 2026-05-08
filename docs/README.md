# Auberge

> Ansible-powered VPS management without Docker bloat.

Auberge is a Rust CLI for managing self-hosted infrastructure. Deploys a full FOSS stack on a 2 GB VPS using native systemd — no containers.

```bash
cargo install auberge
auberge host add my-vps 203.0.113.10
auberge deploy --all --host my-vps
```

That's it. Auberge configures hardening, infrastructure, and applications.

## What you get

| Layer          | Components                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hardening      | UFW, fail2ban, kernel sysctl, SSH on a custom port                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Infrastructure | [Caddy](applications/infrastructure/caddy.md) (auto-HTTPS), [Tailscale](applications/networking/tailscale.md), [Blocky](applications/networking/blocky.md) (DNS + ad-blocking), [Headscale](applications/networking/headscale.md), [WireGuard](applications/networking/wireguard.md)                                                                                                                                                                                                |
| Apps           | [Baikal](applications/apps/baikal.md), [Bichon](applications/apps/bichon.md), [FreshRSS](applications/apps/freshrss.md), [Navidrome](applications/apps/navidrome.md), [Calibre](applications/apps/calibre.md), [Grimmory](applications/apps/grimmory.md), [Paperless-ngx](applications/apps/paperless.md), [WebDAV](applications/apps/webdav.md), [YOURLS](applications/apps/yourls.md), [Syncthing](applications/apps/syncthing.md), [Colporteur](applications/apps/colporteur.md) |
| Notifications  | [TGTG Bot](applications/apps/tgtg.md) (Too Good To Go via Telegram)                                                                                                                                                                                                                                                                                                                                                                                                                 |
| AI             | [Hermes](applications/apps/hermes.md) (self-improving Telegram agent)                                                                                                                                                                                                                                                                                                                                                                                                               |

## Where to start

- [Quick Start](getting-started/quick-start.md) — 5-minute walkthrough
- [First Deployment](getting-started/first-deployment.md) — full setup with config + DNS
- [CLI Reference](cli-reference/auberge.md) — every command
- [Backup & Restore](backup-restore/overview.md) — data protection and migration

## Requirements

- VPS with root/sudo access (Linux, 2 GB RAM minimum, 4 GB recommended with Grimmory)
- Linux or macOS workstation (Windows not supported — use WSL2)
- Cloudflare account (optional, for managed DNS)

## Philosophy

_Selfware_ — direct control, no abstraction layers, no container runtime, transparent operations.

[GitHub](https://github.com/sripwoud/auberge) · [Issues](https://github.com/sripwoud/auberge/issues) · [Contributing](development/contributing.md)
