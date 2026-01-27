# SSH Problems

Troubleshooting SSH connectivity issues.

## Connection Refused

**Symptom:** `ssh: connect to host X port Y: Connection refused`

**Causes and solutions:**

### Wrong SSH port

```bash
# Check configured port
mise env | grep SSH_PORT

# Try connecting with correct port
ssh -p $SSH_PORT ansible@vps-ip
```

### Firewall blocking port

```bash
# Check provider firewall in control panel
# Ensure custom SSH port is allowed
```

### SSH service not running

```bash
# Connect via provider console (VNC)
sudo systemctl status sshd
sudo systemctl start sshd
```

## Permission Denied

**Symptom:** `Permission denied (publickey)`

**Causes and solutions:**

### SSH key not authorized

```bash
# Verify public key is on server
ssh ansible@vps "cat ~/.ssh/authorized_keys"

# Re-add key manually (via provider console)
cat ~/.ssh/identities/ansible_vps.pub  # Copy this
# Paste into VPS ~/.ssh/authorized_keys
```

### Wrong SSH key

```bash
# Specify correct key
ssh -i ~/.ssh/identities/ansible_vps ansible@vps-ip -p $SSH_PORT
```

### Key permissions too open

```bash
# Fix permissions
chmod 600 ~/.ssh/identities/ansible_vps
```

### Wrong user

```bash
# Use correct user
ssh ansible@vps-ip  # Not root after bootstrap
```

## Host Key Verification Failed

**Symptom:** `Host key verification failed`

**Causes:**

- VPS was reinstalled (new host key)
- Man-in-the-middle attack (rare)

**Solution:**

```bash
# Remove old host key
ssh-keygen -R vps-ip
ssh-keygen -R [vps-ip]:custom-port

# Reconnect (will add new key)
ssh ansible@vps-ip
```

## Connection Timeout

**Symptom:** `ssh: connect to host X port Y: Connection timed out`

**Causes and solutions:**

### Wrong IP address

```bash
# Verify IP
ping vps-ip

# Check inventory
mise env | grep AUBERGE_HOST
```

### VPS offline

```bash
# Check VPS status in provider control panel
# Restart VPS if needed
```

### Network issues

```bash
# Test connectivity
ping vps-ip
traceroute vps-ip

# Try from different network
```

## Stale Control Sockets

**Symptom:** Ansible hangs or fails with "ControlPersist" error

**Solution:**

```bash
# Remove stale sockets
rm -rf ~/.ssh/ctl-*
rm -rf /tmp/ssh-*

# Retry connection
ssh ansible@vps-ip
```

## After Bootstrap Lockout

**Symptom:** Cannot connect after running bootstrap

**Causes:**

1. Provider firewall not configured
2. SSH port change failed
3. SSH keys not deployed

**Recovery:**

```bash
# Access via provider console (VNC/serial)

# Check SSH is running
sudo systemctl status sshd

# Check SSH port
sudo grep "^Port" /etc/ssh/sshd_config

# Check firewall
sudo ufw status

# If locked out:
# Option 1: Disable firewall temporarily
sudo ufw disable

# Option 2: Reset to port 22
sudo sed -i 's/^Port.*/Port 22/' /etc/ssh/sshd_config
sudo systemctl restart sshd
sudo ufw allow 22/tcp
sudo ufw enable

# Fix issues, then re-run bootstrap
```

## SSH Debugging

Enable verbose output:

```bash
# Level 1 (basic)
ssh -v ansible@vps-ip

# Level 2 (detailed)
ssh -vv ansible@vps-ip

# Level 3 (very detailed)
ssh -vvv ansible@vps-ip
```

Common debug info:

- Which keys are tried
- Authentication methods
- Connection progress
- Error details

## Ansible SSH Issues

### Ansible cannot connect

```bash
# Test with ansible directly
ansible -i ansible/inventory.yml vps -m ping

# Run with verbose
auberge ansible run --host vps -vvv
```

### Wrong ansible_user

Check `ansible/inventory.yml`:

```yaml
ansible_user: ansible # Should be 'ansible' not 'root' after bootstrap
```

## Related Pages

- [SSH Keys Configuration](configuration/ssh-keys.md)
- [Bootstrap](deployment/bootstrap.md)
- [Ansible Inventory](configuration/ansible-inventory.md)
