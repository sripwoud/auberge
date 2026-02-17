# Ansible Errors

Common Ansible playbook failures and fixes.

## Task Failures

### "failed: UNREACHABLE!"

**Problem:** Cannot connect to VPS via SSH.

**Solution:** See [SSH Problems](troubleshooting/ssh-problems.md)

### "failed: [host] => Permission denied"

**Problem:** Insufficient sudo privileges.

**Solutions:**

```bash
# Verify ansible user has passwordless sudo
ssh ansible@vps "sudo -n true"

# Re-bootstrap if needed
auberge ansible bootstrap my-vps --ip 203.0.113.10
```

### "apt lock" errors

**Problem:** Another apt process running.

**Solutions:**

```bash
# Wait a minute and retry

# Or kill apt processes (via provider console)
sudo killall apt apt-get
sudo rm /var/lib/apt/lists/lock
sudo rm /var/cache/apt/archives/lock
sudo rm /var/lib/dpkg/lock*
```

### "Package not found"

**Problem:** Package unavailable in apt sources.

**Solutions:**

```bash
# Update apt cache
ssh ansible@vps "sudo apt update"

# Check package name
ssh ansible@vps "apt-cache search package-name"
```

## Systemd Failures

### "Service failed to start"

**Check logs:**

```bash
ssh ansible@vps "journalctl -u service-name -n 50"
```

**Common causes:**

- Config file syntax error
- Port already in use
- Missing dependencies
- File permissions

**Solutions:**

```bash
# Validate config
# Check port availability
ssh ansible@vps "sudo ss -tulpn | grep PORT"

# Fix permissions
auberge ansible run --host vps --tags service-name
```

### "Unit not found"

**Problem:** Systemd service file doesn't exist.

**Solution:**

```bash
# Re-run ansible to create service
auberge ansible run --host vps --tags service-name
```

## Templating Errors

### "Undefined variable"

**Problem:** Required variable not set.

**Solution:**

```bash
# Check environment variables
mise env | grep VARIABLE_NAME

# Set if missing
mise set --age-encrypt --prompt VARIABLE_NAME
```

### "template error"

**Problem:** Jinja2 template syntax error.

**Solution:**

- Check template in `ansible/roles/{role}/templates/`
- Verify variable names
- Test template syntax

## File/Directory Issues

### "No such file or directory"

**Problem:** Path doesn't exist.

**Solutions:**

```bash
# Let ansible create it
# Check tasks create parent directories first

# Or create manually
ssh ansible@vps "sudo mkdir -p /path/to/dir"
```

### "File exists"

**Problem:** File creation when file exists.

**Solution:**

- Use `state: present` instead of `state: touch`
- Or use `force: yes` to overwrite

## Network Issues

### "Connection timeout" downloading files

**Problem:** Slow or failed downloads.

**Solutions:**

```bash
# Retry ansible (idempotent)
auberge ansible run --host vps --tags role-name

# Check VPS network
ssh ansible@vps "ping -c 3 8.8.8.8"

# Download manually and scp to VPS if needed
```

### "SSL certificate verify failed"

**Problem:** Certificate validation failed.

**Solutions:**

```bash
# Update CA certificates
ssh ansible@vps "sudo apt update && sudo apt install -y ca-certificates"

# Or bypass validation (not recommended)
# Add to task: validate_certs: no
```

## Handler Issues

### Handler not running

**Problem:** Expected service restart didn't happen.

**Causes:**

- Task didn't report "changed"
- Handler syntax error
- Wrong handler name in notify

**Debug:**

```bash
# Check if task was marked as "changed"
# Manually restart
ssh ansible@vps "sudo systemctl restart service-name"
```

## Idempotency Issues

### Task always shows "changed"

**Problem:** Task not idempotent.

**Common culprits:**

- Using `command` or `shell` modules
- Not using `creates` or `removes` parameters
- File permissions changing unnecessarily

**Solutions:**

```bash
# Use appropriate module (file, package, service)
# Add changed_when: false for read-only commands
```

## Debug Techniques

### Verbose output

```bash
# Level 1
auberge ansible run --host vps -v

# Level 2 (detailed)
auberge ansible run --host vps -vv

# Level 3 (very detailed)
auberge ansible run --host vps -vvv
```

### Check mode

```bash
# Dry run to see what would change
auberge ansible check --host vps --playbook playbooks/apps.yml
```

### Run specific task

```bash
# Use tags to isolate
auberge ansible run --host vps --tags specific-tag
```

### Manual execution

```bash
# Run ansible-playbook directly
cd ansible
ansible-playbook -i inventory.yml playbooks/apps.yml -vvv
```

## Recovery

### Playbook partially applied

**Problem:** Playbook stopped mid-execution.

**Solution:**

```bash
# Re-run (idempotent)
auberge ansible run --host vps --playbook playbooks/apps.yml

# Ansible skips successful tasks
# Reruns failed tasks
```

### System in inconsistent state

**Solutions:**

```bash
# Re-run full stack
auberge ansible run --host vps --playbook playbooks/auberge.yml --skip-tags bootstrap

# Or restore from backup
auberge backup restore latest --host vps
```

## Related Pages

- [Running Playbooks](deployment/running-playbooks.md)
- [SSH Problems](troubleshooting/ssh-problems.md)
- [Common Issues](troubleshooting/common-issues.md)
- [Playbooks](core-concepts/playbooks.md)
