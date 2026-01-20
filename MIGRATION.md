# Migration Guide: Making Auberge Public-Ready

This document summarizes the changes made to remove hardcoded personal information and make the repository public-ready.

## Changes Made

### 1. New Configuration System

**Created Files:**

- `config.example.toml` - Template configuration for CLI users
- `src/config.rs` - Rust configuration loader module
- `ansible/group_vars/all/user.yml` - Ansible user configuration variables

**What Changed:**

- Added TOML-based configuration system for CLI
- Configuration file is gitignored (`config.toml`)
- Users copy `config.example.toml` to `config.toml` and customize

### 2. Rust Code Changes

**Modified Files:**

- `Cargo.toml` - Added `toml = "0.8"` dependency
- `src/main.rs` - Added config module
- `src/commands/sync.rs` - Now reads username from config
- `src/commands/ssh.rs` - Updated help text to be generic
- `src/services/dns.rs` - Reads from config instead of `inventory/dns.yml`

**Removed Files:**

- `src/models/dns.rs` - Replaced by `config::DnsConfig`
- `src/models.rs` - Removed dns module reference

### 3. Ansible Variable Parameterization

**Updated Files:**

- `ansible/inventory.yml` - Uses `{{ vault_admin_user_name }}` variable
- `ansible/inventory/dns.yml` - Uses `{{ vault_primary_domain }}` variable
- `ansible/group_vars/all/hosts.vault.example` - Sanitized IPs, added new variables

**Role Defaults Updated:**

- `ansible/roles/ansible_user/defaults/main.yml`
- `ansible/roles/blocky/defaults/main.yml`
- `ansible/roles/freshrss/defaults/main.yml`
- `ansible/roles/navidrome/defaults/main.yml`
- `ansible/roles/radicale/defaults/main.yml`
- `ansible/roles/webdav/defaults/main.yml`

**Role Tasks Updated:**

- `ansible/roles/blocky/tasks/main.yml` - NAMECHEAP_API_USER now variable

**Templates Updated:**

- `ansible/roles/blocky/templates/lego-renew.service.j2`
- `ansible/roles/caddy/templates/Caddyfile.j2` (converted from files/)
- `ansible/roles/webdav/templates/Caddyfile.j2`
- `ansible/roles/yourls/templates/yourls.caddyfile.j2` (converted from files/)
- `ansible/roles/ssh/templates/sshd_config.j2`

**Comments Updated:**

- `ansible/group_vars/all/freshrss.yml`

### 4. Example Files Sanitized

**Changed:**

- `ansible/group_vars/all/hosts.vault.example`:
  - IPs changed to RFC 5737 documentation addresses (203.0.113.x)
  - Added placeholder variables for username, email, domain
  - Renamed password variable to be generic

### 5. Documentation Updates

**Updated:**

- `README.md` - Added comprehensive setup instructions:
  - Configuration file setup
  - Ansible vault encryption steps
  - Environment variable requirements
  - Build instructions

### 6. Gitignore Updates

**Added:**

- `config.toml` - User-specific CLI configuration

## New Variables in Ansible Vault

Users need to add these to `ansible/group_vars/all/hosts.vault`:

```yaml
vault_admin_user_name: "your-username"
vault_admin_user_email: "you@example.com"
vault_primary_domain: "example.com"
vault_namecheap_api_user: "your-namecheap-username"
vault_admin_become_password: "your-sudo-password"
```

## Required Environment Variables

```bash
NAMECHEAP_API_KEY="your-api-key"
NAMECHEAP_CLIENT_IP="your-public-ip"
```

## For Existing Users

If you're already using this repo, you need to:

1. Create `config.toml` from `config.example.toml`
2. Update your `ansible/group_vars/all/hosts.vault` to add new variables:
   - `vault_admin_user_name`
   - `vault_admin_user_email`
   - `vault_primary_domain`
   - `vault_namecheap_api_user`
3. Re-encrypt your vault file: `ansible-vault encrypt ansible/group_vars/all/hosts.vault`
4. Rebuild the CLI: `cargo build --release`

## Security Notes

All sensitive data remains encrypted:

- Ansible vault files (.vault) are gitignored
- Age-encrypted secrets in `.mise/config.toml` (already encrypted)
- User config.toml is gitignored
- SSH keys are referenced via env vars, never committed

## What Remains Hardcoded (By Design)

- Default SSH key paths (`~/.ssh/identities/`)
- Remote music path (`/srv/music/`)
- Service ports (configurable per role if needed)
