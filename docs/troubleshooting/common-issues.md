# Common Issues (FAQ)

Frequently encountered issues and solutions.

## Installation Issues

### "cargo install auberge" fails

**Problem:** Rust compiler error during installation.

**Solution:**

```bash
# Update Rust
rustup update

# Retry installation
cargo install auberge
```

### mise not found

**Problem:** mise not installed or not in PATH.

**Solution:**

```bash
# Install mise
curl https://mise.run | sh

# Or via package manager
brew install mise  # macOS
```

## Configuration Issues

### Environment variables not set

**Problem:** Commands fail with "environment variable not set".

**Solution:**

```bash
# Check environment
mise env | grep VARIABLE_NAME

# Set if missing
mise set --age-encrypt --prompt VARIABLE_NAME
```

### age encryption fails

**Problem:** "age: error: no identity file".

**Solution:**

```bash
# Generate age key
age-keygen -o ~/.config/age/key.txt

# Verify
ls -l ~/.config/age/key.txt
```

## Host Management Issues

### "Host not found"

**Problem:** Host not in inventory or hosts.toml.

**Solution:**

```bash
# Add host
auberge host add my-vps 203.0.113.10

# Or check inventory
ansible-inventory -i ansible/inventory.yml --list
```

### Cannot edit hosts.toml

**Problem:** File doesn't exist.

**Solution:**

```bash
# Create directory
mkdir -p ~/.config/auberge

# Add first host
auberge host add my-vps 203.0.113.10
```

## Backup Issues

### "No backups found"

**Problem:** Backup directory doesn't exist or is empty.

**Solution:**

```bash
# Create first backup
auberge backup create --host my-vps

# Or check backup directory
ls -la ~/.local/share/auberge/backups/
```

### Backup hangs

**Problem:** SSH connection or rsync stalled.

**Solution:**

```bash
# Test SSH connection
ssh -i ~/.ssh/identities/ansible_my-vps ansible@vps-ip

# Kill stale control sockets
rm -rf ~/.ssh/ctl-*
```

## Deployment Issues

### Playbook fails with "Unreachable"

**Problem:** Cannot connect to VPS via SSH.

**Solution:** See [SSH Problems](troubleshooting/ssh-problems.md)

### Task fails with "Permission denied"

**Problem:** Insufficient sudo privileges.

**Solution:**

```bash
# Verify ansible user has passwordless sudo
ssh ansible@vps-host "sudo -n true"

# Re-run bootstrap if needed
auberge ansible bootstrap my-vps --ip 203.0.113.10
```

### Handler not running

**Problem:** Service not restarted after config change.

**Solution:**

- Check if config task actually changed (marked as "changed")
- Manually restart:
  ```bash
  ssh ansible@vps "sudo systemctl restart service-name"
  ```

## DNS Issues

### "Authentication error"

**Problem:** Invalid Cloudflare API token.

**Solution:**

```bash
# Regenerate token in Cloudflare Dashboard
# Update mise
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
```

### DNS records not propagating

**Problem:** DNS cache or TTL delay.

**Solution:**

```bash
# Wait 5 minutes (default TTL)
# Flush local DNS cache
sudo systemd-resolve --flush-caches  # Linux
sudo dscacheutil -flushcache         # macOS

# Test with specific DNS server
dig @1.1.1.1 subdomain.example.com +short
```

## Application Issues

### Service won't start

**Problem:** systemd service fails to start.

**Solution:**

```bash
# Check service status
ssh ansible@vps "systemctl status service-name"

# Check logs
ssh ansible@vps "journalctl -u service-name -n 50"

# Common fixes:
# - Check config file syntax
# - Verify port not already in use
# - Check file permissions
```

### HTTPS certificate errors

**Problem:** SSL/TLS certificate invalid.

**Solution:**

```bash
# Check Caddy logs
ssh ansible@vps "journalctl -u caddy -n 50"

# Common causes:
# - Cloudflare API token incorrect
# - DNS not pointing to VPS
# - Port 80/443 blocked

# Force certificate renewal
ssh ansible@vps "sudo systemctl restart caddy"
```

## Performance Issues

### Slow playbook execution

**Problem:** Tasks take long time.

**Solution:**

- SSH ControlMaster already enabled (check inventory.yml)
- Use tags to run only needed tasks:
  ```bash
  auberge ansible run --tags specific-app
  ```
- Check VPS resources (CPU/memory)

### Large backup size

**Problem:** Backups consuming too much space.

**Solution:**

```bash
# Exclude music files (largest component)
auberge backup create --host my-vps  # Already excludes music by default

# Check backup sizes
du -sh ~/.local/share/auberge/backups/*/*
```

## Getting Help

Can't find your issue?

1. Check [troubleshooting section](troubleshooting/ssh-problems.md)
2. Search [GitHub issues](https://github.com/sripwoud/auberge/issues)
3. Open new issue with:
   - Auberge version (`auberge --version`)
   - Error message
   - Steps to reproduce

## Related Pages

- [SSH Problems](troubleshooting/ssh-problems.md)
- [Backup/Restore Issues](troubleshooting/backup-restore-issues.md)
- [Ansible Errors](troubleshooting/ansible-errors.md)
- [DNS Issues](troubleshooting/dns-issues.md)
