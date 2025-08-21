# Selfhost Infrastructure

Ansible playbooks and roles for managing self-hosted services and server infrastructure.

## Prerequisites

- Ansible installed locally
- SSH access to target servers
- Ansible Vault password for encrypted variables

## Usage

### Bootstrap New VPS

Use `mise run ansible-bootstrap` for initial VPS setup (first run only):

```bash
mise run ansible-bootstrap
# 1) Select host to bootstrap
# 2) Connects as root with password authentication
# 3) Creates user account and secures SSH
```

**Bootstrap process:**

- Creates non-root user with sudo privileges
- Disables root login and password authentication
- Sets up SSH key-based authentication

### Interactive Playbook Execution

Use `mise run ansible-run` to interactively select both the host and playbook to execute:

```bash
mise run ansible-run
# 1) Select host: lechuck-cloud:194.164.53.11
# 2) Select playbook: bootstrap, site, selfhosted, vibecoder
# 3) Runs ansible-playbook with your selections
```

## Available Playbooks

| Playbook                                   | Description                                                                                                 |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| [bootstrap.yml](playbooks/bootstrap.yml)   | Initial VPS setup - creates users and secures SSH. Run once when provisioning a new server.                 |
| [site.yml](playbooks/site.yml)             | Core server infrastructure including security hardening, firewall, reverse proxy, and system configuration. |
| [selfhosted.yml](playbooks/selfhosted.yml) | Self-hosted applications: Calibre, Blocky DNS, Radicale, WebDAV, YOURLS.                                    |
| [vibecoder.yml](playbooks/vibecoder.yml)   | Development environment setup with Vibecoder and Claude Code integration.                                   |
