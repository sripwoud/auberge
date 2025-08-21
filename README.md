# Selfhost Infrastructure

Ansible playbooks and roles for managing self-hosted services and server infrastructure.

## Prerequisites

- Ansible installed locally
- SSH access to target servers
- Ansible Vault password for encrypted variables

## Playbooks

### bootstrap.yml - Initial VPS Setup (First Run Only)
Bootstraps a fresh VPS by creating users and securing SSH. Run this ONCE when provisioning a new server.

```bash
# Connect as root with password to bootstrap
ansible-playbook -i ansible/inventory.yml playbooks/bootstrap.yml --ask-pass
```

**Included roles:**
- `user` - Create non-root user with sudo privileges
- `ssh` - Disable root login, setup key-based authentication

### site.yml - Base Server Setup
Sets up core server infrastructure including security hardening, firewall, SSH configuration, and reverse proxy.

```bash
# Run complete server setup (after bootstrap)
ansible-playbook -i ansible/inventory.yml playbooks/site.yml --ask-vault-pass

# Run specific roles with tags
ansible-playbook -i ansible/inventory.yml playbooks/site.yml --tags "ssh,ufw" --ask-vault-pass
```

**Included roles:**
- `apt` - Package management and updates
- `user` - User account creation and sudo configuration  
- `ssh` - SSH hardening and key management
- `bash` - Shell configuration
- `kernel_hardening` - System security settings
- `ufw` - Firewall rules
- `fail2ban` - Intrusion prevention
- `tailscale` - Mesh VPN
- `caddy` - Reverse proxy and web server

### selfhosted.yml - Self-Hosted Applications
Deploys and configures self-hosted applications.

```bash
# Deploy all applications
ansible-playbook -i ansible/inventory.yml playbooks/selfhosted.yml --ask-vault-pass

# Deploy specific applications
ansible-playbook -i ansible/inventory.yml playbooks/selfhosted.yml --tags "calibre,radicale" --ask-vault-pass
```

**Available applications:**
- `calibre` - E-book server (lire.riou.one)
- `blocky` - DNS ad-blocker with DoT (dns.sripwoud.xyz)
- `radicale` - CalDAV/CardDAV server (radicale.sripwoud.xyz)
- `webdav` - File sharing server
- `yourls` - URL shortener (url.sripwoud.xyz)

### vibecoder.yml - Development Environment
Interactive setup for Vibecoder development environment with Claude Code integration.

```bash
ansible-playbook -i ansible/inventory.yml playbooks/vibecoder.yml --ask-vault-pass
```

## Common Usage Patterns

### New VPS Setup
```bash
# 1. Bootstrap (first time only, as root)
mise run ansible-bootstrap

# 2. Run base infrastructure (as your user)
mise run ansible-site

# 3. Deploy applications
mise run ansible-apps
```

### Existing Server Updates
```bash
# Update infrastructure
mise run ansible-site

# Update applications
mise run ansible-apps
```

### Updating Specific Services
```bash
# Update only Calibre
ansible-playbook -i ansible/inventory.yml playbooks/selfhosted.yml --tags calibre --ask-vault-pass

# Update security components
ansible-playbook -i ansible/inventory.yml playbooks/site.yml --tags "ufw,fail2ban" --ask-vault-pass
```

### Dry Run (Check Mode)
```bash
# Preview changes without applying them
ansible-playbook -i ansible/inventory.yml playbooks/site.yml --check --ask-vault-pass
```

## Directory Structure

```
selfhost/
├── ansible/
│   ├── ansible.cfg         # Ansible configuration
│   ├── inventory.yml       # Server inventory
│   ├── group_vars/         # Group variables and vault
│   └── roles/              # All Ansible roles
├── playbooks/
│   ├── site.yml           # Base infrastructure playbook
│   ├── selfhosted.yml     # Applications playbook
│   └── vibecoder.yml      # Dev environment playbook
├── mise.toml              # Task runner configuration
└── setup                  # Bootstrap script
```