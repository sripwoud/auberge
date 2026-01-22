# Auberge

Selfware for managing my self-hosted FOSS stack, built around Ansible automation. No Docker bloat, runs lean on a 1€/month 1GB VPS.

## Stack

### Infrastructure

| Name                                             | Description                        |
| ------------------------------------------------ | ---------------------------------- |
| [Caddy](https://caddyserver.com)                 | Reverse proxy with automatic HTTPS |
| [fail2ban](https://github.com/fail2ban/fail2ban) | Intrusion prevention system        |
| [UFW](https://launchpad.net/ufw)                 | Uncomplicated firewall             |

### Apps

| Name                                        | Description                          |
| ------------------------------------------- | ------------------------------------ |
| [Blocky](https://0xerr0r.github.io/blocky)  | DNS server with ad/tracking blocking |
| [WireGuard](https://wireguard.com)          | Fast, modern VPN                     |
| [Tailscale](https://tailscale.com)          | Mesh VPN for secure remote access    |
| [Radicale](https://radicale.org)            | Lightweight CalDAV/CardDAV server    |
| [WebDAV](https://github.com/hacdias/webdav) | File sharing and synchronization     |
| [Calibre](https://calibre-ebook.com)        | Ebook library management             |
| [Navidrome](https://navidrome.org)          | Music streaming server               |
| [FreshRSS](https://freshrss.org)            | RSS feed aggregator                  |
| [YOURLS](https://yourls.org)                | URL shortener                        |

## Develop

See [develop.md](develop.md) for local development setup.

## Prerequisites

- Ansible installed locally
- SSH access to target servers
- Rust toolchain (for building the CLI)

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

> [!IMPORTANT]
> **Before running the apps playbook**, you MUST generate a Cloudflare API token for DNS management.
>
> The certificate automation (Lego) runs from your VPS and uses Cloudflare's API for DNS-01 challenges.
>
> **Required steps:**
>
> 1. Log into Cloudflare: https://dash.cloudflare.com
> 2. Navigate to: **My Profile → API Tokens → Create Token**
> 3. Use the **"Edit zone DNS"** template
> 4. Configure permissions:
>    - Zone → DNS → Edit
>    - Zone → Zone → Read
> 5. Set zone resources to your domain (e.g., sripwoud.xyz)
> 6. No IP filtering needed (unlike Namecheap)
> 7. Copy the token (shown only once)
> 8. Add to `mise.toml` as `CLOUDFLARE_DNS_API_TOKEN`
>
> **Why this is needed:**
>
> - Lego (certificate tool) runs from your VPS, not your local machine
> - It makes Cloudflare API calls for DNS-01 challenge validation
> - Token-based auth (simpler than Namecheap's IP whitelisting)

## Initial Setup

### 1. Environment Variables (Single Source of Truth)

All configuration is managed via `mise.toml` environment variables:

```bash
# Set encrypted secrets using mise + age (will prompt for value)
mise set --age-encrypt --prompt ADMIN_USER_NAME
mise set --age-encrypt --prompt ADMIN_USER_EMAIL
mise set --age-encrypt --prompt PRIMARY_DOMAIN
mise set --age-encrypt --prompt NAMECHEAP_API_KEY
mise set --age-encrypt --prompt NAMECHEAP_API_USER
mise set --age-encrypt --prompt NAMECHEAP_CLIENT_IP
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
mise set --age-encrypt --prompt RADICALE_PASSWORD
mise set --age-encrypt --prompt WEBDAV_PASSWORD
mise set --age-encrypt --prompt TAILSCALE_AUTHKEY
mise set --age-encrypt --prompt SSH_PORT

# Set VPS host IPs
mise set --age-encrypt --prompt AUBERGE_HOST
mise set --age-encrypt --prompt VIBECODER_HOST

# Public config (already in mise.toml [env] section)
# DNS_DEFAULT_TTL = "300"
# BLOCKY_SUBDOMAIN = "dns"
# CALIBRE_SUBDOMAIN = "lire"
# FRESHRSS_SUBDOMAIN = "rss"
# NAVIDROME_SUBDOMAIN = "musique"
# RADICALE_SUBDOMAIN = "calendrier"
# WEBDAV_SUBDOMAIN = "webdav"
# YOURLS_SUBDOMAIN = "url"
```

### 2. Optional: CLI-Only DNS Configuration

If using CLI DNS commands (not Ansible), create `config.toml`:

```bash
cp config.example.toml config.toml
```

Edit `config.toml` with:

- `dns.domain`: Your primary domain (e.g., example.com)
- `dns.default_ttl`: DNS record TTL in seconds (default: 300)
- `cloudflare.zone_id`: (Optional) Your Cloudflare zone ID for performance

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
