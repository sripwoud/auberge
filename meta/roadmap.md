# Roadmap

Feature ideas and directions. Not commitments — items here may become GitHub issues when ready.

## CLI

- Interactive host selection with arrow keys
- Better error messages with suggestions
- Progress bars for long operations
- Color output configuration

## Backup

- Automatic retention policy (keep last N backups)
- Backup compression options
- Remote backup storage (S3, Backblaze B2)
- Backup verification/integrity checks
- Encrypted backups
- Differential backups
- Automatic scheduled backups

## DNS

- Route53 support
- DigitalOcean DNS
- Generic DNS provider abstraction

## Applications

- Nextcloud (file sync + apps)
- Vaultwarden (password manager)
- Jellyfin (media server)
- Gitea (Git hosting)
- Matrix (chat server)

## Infrastructure

- Monitoring (Prometheus + Grafana)
- Alerting (via email or webhook)
- Automatic updates configuration
- Health check endpoints

## Long-Term

- Multi-VPS management from single CLI
- Load balancing between VPS
- Automatic failover
- Distributed backup storage

## Under Consideration

- Docker support as alternative to systemd
- Podman integration
- FreeBSD support
- Raspberry Pi optimizations
- Automatic SSL certificate rotation
- Security scanning integration

## Probably Not

- **Windows VPS support** — systemd dependency
- **Multi-tenancy** — designed for personal use
- **GUI installer** — CLI-first philosophy
- **Proprietary cloud integrations** — FOSS priority
- **Web UI** — CLI-first philosophy
