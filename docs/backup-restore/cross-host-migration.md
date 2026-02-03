# Cross-Host Migration

The `--from-host` flag enables restoring backups from one host to a different host. This is useful for:

- **VPS Provider Migration**: Moving from one provider to another (e.g., IONOS → Hetzner)
- **Disaster Recovery**: Restoring to a fresh VPS after hardware failure
- **Dev/Test Environments**: Seeding staging with production-like data

## Basic Usage

```bash
auberge backup restore latest --from-host old-vps --host new-vps
```

Restore specific apps only:

```bash
auberge backup restore latest --from-host old-vps --host new-vps --apps baikal,freshrss
```

Dry run to preview cross-host restore:

```bash
auberge backup restore latest --from-host old-vps --host new-vps --dry-run
```

## Safety Features

Cross-host restore includes comprehensive safety checks to prevent data loss and ensure successful migration:

### 1. Pre-flight Validation

Before any restore operation, Auberge validates:

- **SSH Connectivity**: Tests connection to target host with 10-second timeout
- **Service Existence**: Verifies all required systemd services are installed on target
- **Disk Space**: Ensures target has sufficient space (requires 120% of backup size)

If any validation fails, the restore is aborted with a clear error message.

### 2. Hostname Confirmation

Cross-host restores require typing the target hostname to confirm:

```
Type the target host name 'new-vps' to confirm: new-vps
```

This prevents accidental cross-host restores that could overwrite the wrong server.

### 3. Emergency Backup

Before overwriting data on the target host, Auberge automatically creates an emergency backup:

```
--- Creating Emergency Backup ---
  Backing up current state of 'new-vps' before cross-host restore
  ✓ Emergency backup created: pre-migration-2026-01-23_15-30-00
    Location: ~/.local/share/auberge/backups/new-vps/{app}/2026-01-23_15-30-00
```

If emergency backup creation fails, you'll be prompted whether to continue without it.

### 4. Escape Hatch with --yes Flag

When using `--yes` to skip confirmation prompts, cross-host restores add a 3-second delay:

```bash
auberge backup restore latest --from-host old-vps --host new-vps --yes
```

```
⚠  Cross-host restore with --yes flag
   Waiting 3 seconds (press Ctrl+C to cancel)...
```

This provides time to abort (Ctrl+C) if the command was run accidentally.

## Post-Restore Actions

After a successful cross-host restore, manual verification and configuration updates are typically required.

### Service Verification

Check that services are running:

```bash
ssh user@new-vps 'systemctl status php*-fpm freshrss navidrome'
```

Check logs for errors:

```bash
ssh user@new-vps 'journalctl -u php*-fpm --since "5 minutes ago" | grep -i error'
```

### Configuration Updates

Re-run Ansible to regenerate host-specific configurations:

```bash
auberge ansible run --host new-vps --tags baikal,freshrss,navidrome
```

### DNS Updates

If the hostname changed, update DNS records to point to the new host:

```bash
auberge dns set-all --host new-vps
```

### SSL Certificates

Verify SSL certificates are valid for the new domain. Caddy will automatically provision new certificates, but verify they're working:

```bash
curl -I https://cal.example.com
```

### App-Specific Notes

**Baikal** (CalDAV/CardDAV):

- Data lives in `/opt/baikal/Specific`; verify admin and DAV users in the Baikal web admin after restore.

**Navidrome** (Music Streaming):

- May need to rescan music library if file paths changed
- Trigger rescan from web UI or restart service

**FreshRSS** (RSS Reader):

- Database paths should work, but verify feeds update correctly
- Check feed refresh status in web UI

## Troubleshooting

### "Required service not found on target host"

The target host is missing required systemd services. Run Ansible to install:

```bash
auberge ansible run --host new-vps
```

### "Insufficient disk space"

Target host doesn't have enough space for the restore. Free up space or use a larger VPS.

### Service Won't Start After Restore

Check service logs:

```bash
ssh user@host 'journalctl -u php*-fpm -n 50'
```

Common issues:

- File permissions (automatically fixed during restore; if still broken, re-run Ansible)
- Missing dependencies (fix: reinstall service)
- Port conflicts (fix: check `netstat -tulpn`)

## Related Documentation

- [Restoring Backups](restoring-backups.md) - Same-host restore
- [Best Practices](best-practices.md) - Backup strategies
- [CLI Reference: backup restore](../cli-reference/backup/restore.md) - Full command documentation
