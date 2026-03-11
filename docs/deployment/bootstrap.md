# Bootstrap

First-time VPS setup. Run once on fresh VPS.

## Prerequisites

1. **Root SSH access**
2. **Config values set in `config.toml`:**
   ```bash
   auberge config set ssh_port 22022
   auberge config set admin_user_name yourname
   auberge config set admin_user_email you@example.com
   ```

3. **Provider firewall configured** (critical):
   - Custom SSH port (from `ssh_port` in `config.toml`)
   - 80/tcp, 443/tcp, 853/tcp

   **Why**: Bootstrap changes SSH port. If provider firewall blocks it, you're locked out.

4. **SSH keys generated:**
   ```bash
   auberge ssh keygen --host auberge --user ansible
   auberge ssh keygen --host auberge --user your-username
   ```

## Running

```bash
auberge ansible bootstrap auberge --ip 203.0.113.10
```

## What It Does

1. Sets hostname to inventory name
2. Creates dual-user system:
   - `ansible` - automation user (passwordless sudo)
   - `{admin_user_name}` - personal admin (full sudo)
3. Configures UFW firewall rules (not enabled yet)
4. Hardens SSH (custom port, disable root/password auth)
5. Validates SSH on new port
6. Enables firewall (only after successful validation)

## Post-Bootstrap

```bash
# Test SSH access (replace PORT and HOST with your configured values)
ssh -i ~/.ssh/identities/ansible_auberge -p PORT ansible@HOST

# Continue with next layer
auberge ansible run --tags hardening
```

## Troubleshooting

**"SSH_PORT not set"**

```bash
auberge config set ssh_port 22022
```

**"Connection refused"**

Provider firewall blocks custom port. Add firewall rule in provider control panel.

**Locked out**

Use provider's web console (VNC) to disable firewall (`sudo ufw disable`), fix SSH, re-enable.

**"User already exists"**

Bootstrap already ran. Don't re-run. Use normal playbooks instead.
