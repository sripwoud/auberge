# Roadmap

Future plans and potential features for Auberge.

## Planned Features

### Near-Term (v0.3.x)

**CLI Improvements:**

- Interactive host selection with arrow keys
- Better error messages with suggestions
- Progress bars for long operations
- Color output configuration

**Backup Enhancements:**

- Automatic retention policy (keep last N backups)
- Backup compression options
- Remote backup storage (S3, Backblaze B2)
- Backup verification/integrity checks

**Documentation:**

- Video tutorials
- More examples and recipes
- Troubleshooting flowcharts

### Mid-Term (v0.4.x - v0.6.x)

**Multi-Provider DNS:**

- Cloudflare (current)
- Route53 support
- DigitalOcean DNS
- Generic DNS provider abstraction

**Application Additions:**

- Nextcloud (file sync + apps)
- Vaultwarden (password manager)
- Jellyfin (media server)
- Gitea (Git hosting)
- Matrix (chat server)

**Infrastructure:**

- Monitoring (Prometheus + Grafana)
- Alerting (via email or webhook)
- Automatic updates configuration
- Health check endpoints

**Backup:**

- Encrypted backups
- Differential backups
- S3-compatible remote storage
- Automatic scheduled backups

### Long-Term (v1.0+)

**Multi-VPS Management:**

- Manage multiple VPS from single CLI
- Load balancing between VPS
- Automatic failover
- Distributed backup storage

**Web UI:**

- Dashboard for VPS status
- Point-and-click app deployment
- Backup management interface
- Log viewer

**Advanced Features:**

- Blue-green deployments
- Canary releases
- A/B testing infrastructure
- Auto-scaling (if applicable)

## Under Consideration

### Maybe Features

**Containerization:**

- Docker support as alternative to systemd
- Podman integration
- Kubernetes deployment option

**Platform Support:**

- FreeBSD support
- macOS as deployment target (local services)
- Raspberry Pi optimizations

**Security:**

- Automatic SSL certificate rotation
- Security scanning integration
- Compliance checking
- Vulnerability notifications

### Probably Not

**Features unlikely to be implemented:**

- **Windows VPS support** - systemd dependency
- **Multi-tenancy** - designed for personal use
- **GUI installer** - CLI-first philosophy
- **Proprietary cloud integrations** - FOSS priority

## Contributing to Roadmap

Have ideas? We'd love to hear them.

**How to suggest features:**

1. Open [GitHub Discussion](https://github.com/sripwoud/auberge/discussions)
2. Describe use case and benefits
3. Provide examples if applicable

**What makes a good feature request:**

- ✓ Solves real problem
- ✓ Aligns with project goals
- ✓ Has clear success criteria
- ✓ Considers trade-offs

**What we look for:**

- Maintains simplicity
- Doesn't require major refactoring
- Benefits multiple users
- Has volunteer implementation (or funding)

## Release Cadence

**Current approach:**

- Feature releases: When ready
- Patch releases: As needed for bugs
- No fixed schedule

**Future (post-1.0):**

- Regular monthly releases
- Security patches ASAP
- Deprecation warnings 2 releases ahead

## Version History

See [Changelog](about/changelog.md) for past releases.

## How to Track Progress

**GitHub:**

- [Issues](https://github.com/sripwoud/auberge/issues) - Bugs and features
- [Projects](https://github.com/sripwoud/auberge/projects) - Roadmap board
- [Milestones](https://github.com/sripwoud/auberge/milestones) - Release planning

**Stay Updated:**

- Watch repository on GitHub
- Follow releases
- Join discussions

## Related Pages

- [Changelog](about/changelog.md) - Version history
- [Contributing](development/contributing.md) - How to help
- [Architecture Decisions](about/architecture-decisions.md) - Why we build this way
