# Backup/Restore Issues

Troubleshooting backup and restore problems.

## Backup Creation Fails

### "SSH key not found"

**Solution:**

```bash
# Generate SSH key
auberge ssh keygen --host my-vps --user ansible

# Or specify custom key
auberge backup create --host my-vps --ssh-key /path/to/key
```

### "No such directory"

**Problem:** Service data directory doesn't exist on VPS.

**Solution:**

```bash
# Deploy service first
auberge ansible run --host my-vps --tags radicale

# Then backup
auberge backup create --host my-vps
```

### Backup hangs/freezes

**Causes:**

- Large files being transferred
- SSH connection timeout
- rsync stalled

**Solution:**

```bash
# Kill stale SSH sockets
rm -rf ~/.ssh/ctl-*

# Retry backup
auberge backup create --host my-vps

# For Navidrome, exclude music (large)
# (already excluded by default)
```

### "Permission denied" during backup

**Problem:** Insufficient permissions to read service files.

**Solution:**

```bash
# Verify ansible user has sudo access
ssh ansible@vps "sudo -n ls /var/lib/radicale"

# Re-run ansible to fix permissions
auberge ansible run --host my-vps --tags radicale
```

## Restore Fails

### "No backups found for host"

**Solution:**

```bash
# List available backups
auberge backup list

# Check backup directory
ls -la ~/.local/share/auberge/backups/

# Create backup if missing
auberge backup create --host my-vps
```

### "Service not found on target"

**Problem:** Trying to restore to VPS that doesn't have service installed.

**Solution:**

```bash
# Deploy service first
auberge ansible run --host new-vps --tags radicale

# Then restore
auberge backup restore latest --host new-vps
```

### "Insufficient disk space"

**Problem:** Target VPS doesn't have enough space.

**Solution:**

```bash
# Check disk space
ssh ansible@vps "df -h"

# Free up space or use larger VPS
# Exclude large apps from restore
auberge backup restore latest --host vps --apps radicale,freshrss
```

### Service won't start after restore

**Causes:**

- File permissions incorrect
- Config incompatible with new host
- Port conflicts

**Solution:**

```bash
# Check service logs
ssh ansible@vps "journalctl -u radicale -n 50"

# Re-run ansible to fix permissions/config
auberge ansible run --host vps --tags radicale

# Restart service
ssh ansible@vps "sudo systemctl restart radicale"
```

## Cross-Host Restore Issues

### Pre-flight validation fails

**Problem:** Emergency backup creation fails.

**Solution:**

```bash
# Skip emergency backup (use with caution)
# Not yet implemented - contact maintainer if needed

# Or fix the issue and retry
```

### Hostname confirmation timeout

**Problem:** Can't type hostname quickly enough.

**Solution:**

- Use `--yes` flag to skip (but adds 3-second delay for safety)
- Or just type faster (no actual timeout)

## OPML Import/Export Issues

### "FreshRSS not installed"

**Solution:**

```bash
# Install FreshRSS first
auberge ansible run --host my-vps --tags freshrss

# Then export/import
auberge backup export-opml --host my-vps --output feeds.opml
```

### "Cannot connect to FreshRSS"

**Problem:** FreshRSS service not running.

**Solution:**

```bash
# Check service
ssh ansible@vps "systemctl status freshrss"

# Restart if needed
ssh ansible@vps "sudo systemctl restart freshrss"
```

## Performance Issues

### Backup/restore very slow

**Causes:**

- Large music library
- Slow network connection
- High VPS load

**Solutions:**

```bash
# Exclude music (if not needed)
auberge backup create --host vps  # Music already excluded by default

# Include music only when needed
auberge backup create --host vps --include-music

# Use compression (already enabled by rsync)
```

### Out of memory during backup

**Problem:** Not enough RAM on VPS or local machine.

**Solution:**

```bash
# Backup apps individually
auberge backup create --host vps --apps radicale
auberge backup create --host vps --apps freshrss
```

## Data Integrity

### Verify backup contents

```bash
# List backup directory
ls -lah ~/.local/share/auberge/backups/my-vps/radicale/latest/

# Check file count
find ~/.local/share/auberge/backups/my-vps/radicale/latest/ -type f | wc -l
```

### Compare backup to source

```bash
# Dry run restore to see what would change
auberge backup restore latest --host my-vps --dry-run
```

## Related Pages

- [Backup Overview](backup-restore/overview.md)
- [Creating Backups](backup-restore/creating-backups.md)
- [Restoring Backups](backup-restore/restoring-backups.md)
- [Cross-Host Migration](backup-restore/cross-host-migration.md)
- [SSH Problems](troubleshooting/ssh-problems.md)
