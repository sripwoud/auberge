# Auberge

CLI tool for managing self-hosted infrastructure with Ansible.

## Prerequisites

- Ansible installed locally
- SSH access to target servers
- Rust toolchain (for building the CLI)

## Initial Setup

### 1. Configuration

Copy the example configuration files and customize them:

```bash
# Copy CLI config
cp config.example.toml config.toml

# Copy Ansible vault
cp ansible/group_vars/all/hosts.vault.example ansible/group_vars/all/hosts.vault
```

Edit `config.toml` with your details:

- `username`: Your admin username for SSH and services
- `email`: Your email for Let's Encrypt certificates
- `domain`: Your primary domain (e.g., example.com)
- `namecheap.api_user`: Your Namecheap account username

Edit `ansible/group_vars/all/hosts.vault` with:

- Server IPs
- User configuration (username, email, domain)
- Namecheap API username
- Sudo password

### 2. Encrypt Sensitive Data

```bash
# Create Ansible vault password file
echo "your-vault-password" > ~/.ansible-vault-pass

# Encrypt the vault file
ansible-vault encrypt ansible/group_vars/all/hosts.vault

# Optionally encrypt secrets.vault if you have one
ansible-vault encrypt ansible/group_vars/all/secrets.vault
```

### 3. Environment Variables

Export required environment variables (add to your shell rc file):

```bash
export NAMECHEAP_API_KEY="your-api-key"
export NAMECHEAP_CLIENT_IP="your-public-ip"
```

### 4. Build the CLI

```bash
cargo build --release
# Binary will be at target/release/auberge
```

## Project Structure

```
auberge/
├── src/              # Rust CLI source code
├── ansible/          # Ansible infrastructure
│   ├── playbooks/    # Ansible playbooks
│   ├── roles/        # Ansible roles
│   ├── inventory/    # Dynamic inventory
│   ├── inventory.yml # Static inventory
│   └── ansible.cfg   # Ansible configuration
├── Cargo.toml        # Rust package manifest
└── README.md
```

The CLI wraps Ansible operations, providing an interactive interface for managing infrastructure.

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

| Playbook                                           | Description                                                                                                 |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| [bootstrap.yml](ansible/playbooks/bootstrap.yml)   | Initial VPS setup - creates users and secures SSH. Run once when provisioning a new server.                 |
| [site.yml](ansible/playbooks/site.yml)             | Core server infrastructure including security hardening, firewall, reverse proxy, and system configuration. |
| [selfhosted.yml](ansible/playbooks/selfhosted.yml) | Self-hosted applications: Calibre, Blocky DNS, Radicale, WebDAV, YOURLS.                                    |
| [vibecoder.yml](ansible/playbooks/vibecoder.yml)   | Development environment setup with Vibecoder and Claude Code integration.                                   |
