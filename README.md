# Auberge

CLI tool for managing self-hosted infrastructure with Ansible.

## Prerequisites

- Ansible installed locally
- SSH access to target servers
- Rust toolchain (for building the CLI)

## ⚠️ Important: Provider Firewall Configuration

> [!IMPORTANT]
> **Before running bootstrap**, you MUST configure your VPS provider's firewall to allow your custom SSH port.
>
> Most VPS providers (IONOS, DigitalOcean, Hetzner, AWS, etc.) have their own firewall layer **separate from UFW**.
>
> **Steps:**
>
> 1. Decrypt your `SSH_PORT` value: `mise env | grep SSH_PORT`
> 2. Log into your VPS provider's control panel (e.g., IONOS Dashboard)
> 3. Navigate to firewall/security settings for your VPS
> 4. Add a rule to **allow TCP traffic on your SSH_PORT**
> 5. Save the firewall rule
> 6. Then run the auberge deployment
>
> **Why this is needed:**
>
> - Auberge changes SSH from port 22 → your custom SSH_PORT during bootstrap
> - UFW (on the VPS) will allow the port, but provider firewalls block it by default
> - Without this step, you'll be locked out after SSH port change
>
> **Common provider firewall locations:**
>
> - **IONOS**: Cloud Panel → Server → Firewall
> - **DigitalOcean**: Networking → Firewalls
> - **Hetzner**: Cloud Console → Firewalls
> - **AWS**: Security Groups
> - **Vultr**: Settings → Firewall

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
# 2) Select playbook: bootstrap, hardening, infrastructure, apps, auberge, vibecoder
# 3) Runs ansible-playbook with your selections
```

## Available Playbooks

Playbooks are organized in layers for modular deployment:

| Layer | Playbook                                                   | Description                                                                          |
| ----- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 0     | [bootstrap.yml](ansible/playbooks/bootstrap.yml)           | Initial VPS setup - creates users and secures SSH. Run once per new server.          |
| 1     | [hardening.yml](ansible/playbooks/hardening.yml)           | Security hardening - firewall (UFW), intrusion prevention (fail2ban), kernel.        |
| 2     | [infrastructure.yml](ansible/playbooks/infrastructure.yml) | Core infrastructure - package management (apt), shell (bash), reverse proxy (Caddy). |
| 3     | [apps.yml](ansible/playbooks/apps.yml)                     | Self-hosted applications layer.                                                      |
| -     | [auberge.yml](ansible/playbooks/auberge.yml)               | **Master playbook** - runs all layers (bootstrap → hardening → infra → apps).        |
| -     | [vibecoder.yml](ansible/playbooks/vibecoder.yml)           | Development environment with Vibecoder and Claude Code integration.                  |

### Layer Tags

Each playbook supports hierarchical tagging for granular control:

```bash
# Run entire layer
ansible-playbook auberge.yml --tags hardening

# Run specific function across layers
ansible-playbook auberge.yml --tags security

# Run specific component
ansible-playbook auberge.yml --tags caddy

# Skip bootstrap (already done)
ansible-playbook auberge.yml --skip-tags bootstrap
```
