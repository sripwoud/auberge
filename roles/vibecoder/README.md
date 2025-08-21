# Vibecoder Role

A comprehensive Ansible role for setting up a secure, hardened VPS with Claude Code development environment.

## Features

### Security Hardening

- **SSH**: Hardened configuration with sshusers group, strict timeouts, rate limiting
- **Firewall**: UFW with fail2ban integration, rate limiting on SSH
- **Kernel**: Network stack hardening, memory protection, filesystem security
- **Updates**: Automatic security updates via unattended-upgrades
- **Tailscale**: Private mesh networking for secure remote access

### Development Tools

- **Claude Code**: Full permissions setup with IS_SANDBOX=1
- **Claude Code Remote**: Control Claude remotely via messaging platforms
- **tmux**: Required for Claude Code Remote command injection

## Usage

Add to your playbook:

```yaml
- hosts: your_vps
  roles:
    - vibecoder
```

## Required Variables

```yaml
# User configuration
user_name: "your_username"
ssh_public_key_path: "/path/to/ssh/key.pub"

# Tailscale configuration (optional but recommended)
tailscale_authkey: "tskey-auth-..."
```

## Dependencies

This meta-role includes:

- apt (package management + auto updates)
- user (user creation)
- ssh (hardened SSH)
- bash (shell configuration)
- ufw (firewall)
- fail2ban (intrusion detection)
- kernel_hardening (system security)
- tailscale (mesh networking)
- claude_code (AI development)
- claude_code_remote (remote control)

## Post-Installation

1. **Connect to Tailscale**:
   ```bash
   tailscale up --authkey=YOUR_KEY
   ```

2. **Start Claude Code**:
   ```bash
   tmux new -s claude
   claude
   ```

3. **Configure SSH access via Tailscale only**:
   ```bash
   # Block public SSH access
   sudo ufw delete allow 2209/tcp
   sudo ufw allow in on tailscale0 to any port 2209
   ```

## Security Notes

- SSH is accessible publicly by default, consider restricting to Tailscale only
- All roles include proper hardening and rate limiting
- Automatic security updates are enabled
- Claude Code has full permissions (use with caution)
