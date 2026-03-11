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

## Configuration

All configuration is managed via `config.toml`. Copy `config.example.toml` and fill in values:

```bash
cp config.example.toml config.toml
```

Key values to set:

```bash
auberge config set admin_user_name yourname
auberge config set admin_user_email you@example.com
auberge config set domain example.com
auberge config set cloudflare_dns_api_token your-token
auberge config set baikal_admin_password your-password
auberge config set webdav_password your-password
auberge config set tailscale_authkey your-authkey
auberge config set ssh_port 22022
```

See `config.example.toml` for all available options including subdomain overrides.

## Host Management

Hosts can be managed in two ways:

### Option 1: XDG Config (Recommended for end users)

For users installing via `cargo install`, hosts are managed in `~/.config/auberge/hosts.toml`:

```bash
# Add a host interactively
auberge host add my-vps

# Or non-interactively
auberge host add my-vps 203.0.113.10 --user admin --port 22

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
address = "203.0.113.10"
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
6. Copy the token and add to config:
   ```bash
   auberge config set cloudflare_dns_api_token your-token
   ```

**Note**: IP whitelisting is optional (all IPs are allowed by default)

### Tailscale

Required for VPN mesh networking:

1. Generate auth key at [Tailscale Admin Console](https://login.tailscale.com/admin/settings/keys)
2. Set reusable and ephemeral flags as needed
3. Store the key:
   ```bash
   auberge config set tailscale_authkey your-authkey
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

1. Check your SSH port: `auberge config show`
2. Log into your VPS provider control panel (IONOS, DigitalOcean, Hetzner, etc.)
3. Add firewall rule to allow TCP traffic on your `ssh_port`
4. Save the rule

This prevents lockout when Ansible changes SSH from port 22 to your custom port.
