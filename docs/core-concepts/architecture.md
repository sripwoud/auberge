# Architecture

Auberge: Rust CLI → Ansible → systemd services

## Design Philosophy

**No Docker**: Direct systemd services for minimal resource overhead

**Rust CLI**: Type-safe wrapper around Ansible with safety checks and backup automation

**Ansible**: Idempotent, declarative infrastructure as code

## Layers

1. **Bootstrap** - Initial VPS setup (users, SSH, firewall)
2. **Hardening** - Security (fail2ban, kernel hardening)
3. **Infrastructure** - Core services (apt, bash, Caddy)
4. **Applications** - Self-hosted apps (Baikal, FreshRSS, Navidrome, etc.)

## Data Flow

**Deployment**: CLI → Ansible → SSH → VPS systemd

**Backup**: VPS → SSH/rsync → Local storage

**Restore**: Local → SSH/rsync → VPS

## State Management

**Ansible**: Stateless (checks current state each run)

**Configuration**:

- `~/.config/auberge/hosts.toml` - User-specific hosts (backup/SSH)
- `ansible/inventory.yml` - Shared playbook inventory (version controlled)

**Application data**: Service directories on VPS (`/var/lib/app/`, `/home/user/`)

## Security

- Dual-user system (ansible automation + personal admin)
- Custom SSH port, key-based auth only
- age-encrypted secrets in `mise.toml`
- UFW firewall + fail2ban
- Cloudflare proxy for HTTPS
