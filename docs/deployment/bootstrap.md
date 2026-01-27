# Bootstrap

First-time VPS setup. Run once on fresh VPS.

## Prerequisites

1. **Root SSH access**
2. **Environment variables set:**
   ```bash
   mise set --age-encrypt --prompt SSH_PORT
   mise set --age-encrypt --prompt ADMIN_USER_NAME
   mise set --age-encrypt --prompt ADMIN_USER_EMAIL
   mise set --age-encrypt --prompt AUBERGE_HOST
   ```

3. **Provider firewall configured** (critical):
   - Custom SSH port (from `SSH_PORT`)
   - 80/tcp, 443/tcp, 853/tcp

   **Why**: Bootstrap changes SSH port. If provider firewall blocks it, you're locked out.

4. **SSH keys generated:**
   ```bash
   auberge ssh keygen --host auberge --user ansible
   auberge ssh keygen --host auberge --user your-username
   ```

## Running

```bash
auberge ansible bootstrap auberge --ip 194.164.53.11
```

## What It Does

1. Sets hostname to inventory name
2. Creates dual-user system:
   - `ansible` - automation user (passwordless sudo)
   - `{ADMIN_USER_NAME}` - personal admin (full sudo)
3. Configures UFW firewall rules (not enabled yet)
4. Hardens SSH (custom port, disable root/password auth)
5. Validates SSH on new port
6. Enables firewall (only after successful validation)

## Post-Bootstrap

```bash
# Test SSH access
ssh -i ~/.ssh/identities/ansible_auberge -p $SSH_PORT ansible@$AUBERGE_HOST

# Continue with next layer
auberge ansible run --tags hardening
```

## Troubleshooting

**"SSH_PORT not set"**

```bash
mise set --age-encrypt --prompt SSH_PORT
```

**"Connection refused"**

Provider firewall blocks custom port. Add firewall rule in provider control panel.

**Locked out**

Use provider's web console (VNC) to disable firewall (`sudo ufw disable`), fix SSH, re-enable.

**"User already exists"**

Bootstrap already ran. Don't re-run. Use normal playbooks instead.
