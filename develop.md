# Development Setup

## Prerequisites

- Rust toolchain (1.70+)
- Ansible (for playbook development)
- mise (for environment management)
- age (for encrypting secrets)

## Bootstrap

Run the setup script to install required development tools:

```bash
./setup
```

This installs:

- mise (environment and tool version manager)
- Rust toolchain
- Ansible and ansible-lint
- Other dev dependencies (dprint, dasel, pkl, etc.)

## Environment Variables

All configuration is managed via `mise.toml` environment variables.

### Secrets (encrypted with age)

Set encrypted secrets using mise:

```bash
mise set --age-encrypt --prompt ADMIN_USER_NAME
mise set --age-encrypt --prompt ADMIN_USER_EMAIL
mise set --age-encrypt --prompt PRIMARY_DOMAIN
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
mise set --age-encrypt --prompt RADICALE_PASSWORD
mise set --age-encrypt --prompt WEBDAV_PASSWORD
mise set --age-encrypt --prompt TAILSCALE_AUTHKEY
mise set --age-encrypt --prompt SSH_PORT
mise set --age-encrypt --prompt AUBERGE_HOST
mise set --age-encrypt --prompt VIBECODER_HOST
```

### Public Configuration

Already defined in `mise.toml` [env] section:

- `DNS_DEFAULT_TTL` - DNS record TTL in seconds (default: 300)
- `BLOCKY_SUBDOMAIN` - Subdomain for Blocky DNS (default: dns)
- `CALIBRE_SUBDOMAIN` - Subdomain for Calibre (default: lire)
- `FRESHRSS_SUBDOMAIN` - Subdomain for FreshRSS (default: rss)
- `NAVIDROME_SUBDOMAIN` - Subdomain for Navidrome (default: musique)
- `RADICALE_SUBDOMAIN` - Subdomain for Radicale (default: calendrier)
- `WEBDAV_SUBDOMAIN` - Subdomain for WebDAV (default: webdav)
- `YOURLS_SUBDOMAIN` - Subdomain for YOURLS (default: url)

## Host Management

Hosts can be managed in two ways:

### Option 1: XDG Config (Recommended for end users)

For users installing via `cargo install`, hosts are managed in `~/.config/auberge/hosts.toml`:

```bash
# Add a host interactively
auberge host add my-vps

# Or non-interactively
auberge host add my-vps 194.164.53.11 --user admin --port 22

# List hosts
auberge host list

# Show host details
auberge host show my-vps

# Edit host
auberge host edit my-vps

# Remove host
auberge host remove my-vps
```

The `hosts.toml` format:

```toml
[[hosts]]
name = "auberge"
address = "194.164.53.11"
user = "sripwoud"
port = 22
tags = ["production"]
description = "Main VPS"
```

### Option 2: Ansible Inventory (Recommended for developers)

For development, keep using `ansible/inventory.yml` in the repository:

```yaml
all:
  children:
    vps:
      hosts:
        auberge:
          ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
          ansible_port: 22
          bootstrap_user: root
```

### Priority Order

The CLI checks hosts in this order:

1. `~/.config/auberge/hosts.toml` (if exists and not empty)
2. `ansible/inventory.yml` (fallback for developers)
3. Environment variables (legacy support)

## API Keys and Tokens

### Cloudflare API Token

Required for DNS-01 ACME challenges via Lego certificate automation:

1. Log into [Cloudflare Dashboard](https://dash.cloudflare.com)
2. Navigate to: My Profile → API Tokens → Create Token
3. Use the "Edit zone DNS" template
4. Configure permissions:
   - Zone → DNS → Edit
   - Zone → Zone → Read
5. Set zone resources to your domain
6. Copy the token and add to mise:
   ```bash
   mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
   ```

**Note**: IP whitelisting is optional (all IPs are allowed by default)

### Tailscale

Required for VPN mesh networking:

1. Generate auth key at [Tailscale Admin Console](https://login.tailscale.com/admin/settings/keys)
2. Set reusable and ephemeral flags as needed
3. Store the key:
   ```bash
   mise set --age-encrypt --prompt TAILSCALE_AUTHKEY
   ```

## Building

```bash
mise run build
# or
cargo build --release
```

Binary output: `target/release/auberge`

## Testing

```bash
cargo test
```

## Linting

```bash
mise run lint
# or separately
cargo clippy
ansible-lint
```

## VPS Provider Firewall

Before running bootstrap playbook, configure your VPS provider's firewall to allow your custom SSH port:

1. Decrypt your SSH_PORT: `mise env | grep SSH_PORT`
2. Log into your VPS provider control panel (IONOS, DigitalOcean, Hetzner, etc.)
3. Add firewall rule to allow TCP traffic on your SSH_PORT
4. Save the rule

This prevents lockout when Ansible changes SSH from port 22 to your custom port.
