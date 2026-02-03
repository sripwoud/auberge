# Backup & Restore

## Overview

Auberge provides built-in backup and restore functionality for all self-hosted applications. Backups are stored locally and can be restored to the same host or migrated to a different host using the cross-host restore feature.

## Supported Applications

- **Baikal**: Calendar and contact data, configuration files
- **FreshRSS**: SQLite database, configuration, user data
- **Navidrome**: Database and configuration (music files excluded by default)
- **Calibre**: Book library, metadata database, user database (login credentials)
- **WebDAV**: All shared files

## Backup Storage

Backups are stored locally in `~/.local/share/auberge/backups/` with the following structure:

```
backups/
└── {hostname}/
    ├── baikal/
    │   ├── 2026-01-23_14-30-00/
    │   ├── 2026-01-23_18-45-12/
    │   └── latest -> 2026-01-23_18-45-12
    ├── freshrss/
    ├── navidrome/
    ├── calibre/
    └── webdav/
```

Each app has a `latest` symlink pointing to the most recent backup for easy access.

## Creating Backups

### Backup All Apps

```bash
auberge backup create --host my-vps
```

This creates a timestamped backup of all applications with their data and configuration.

### Backup Specific Apps

```bash
auberge backup create --host my-vps --apps baikal,freshrss
```

### Include Music Files

By default, Navidrome backups exclude the music library (`/srv/music`) to save space and time. To include music files:

```bash
auberge backup create --host my-vps --include-music
```

### Dry Run

Preview what will be backed up without making changes:

```bash
auberge backup create --host my-vps --dry-run
```

### Custom Destination

```bash
auberge backup create --host my-vps --dest /mnt/external/backups
```

## Listing Backups

### List All Backups

```bash
auberge backup list
```

Example output:

```
HOST            APP          TIMESTAMP            SIZE
-----------------------------------------------------------------
my-vps          baikal     2026-01-23_14-30-00  3.24 MB
my-vps          freshrss     2026-01-23_14-30-01  39.57 MB
my-vps          navidrome    2026-01-23_14-30-02  856.06 KB
```

### Filter by Host or App

```bash
auberge backup list --host my-vps
auberge backup list --app baikal
auberge backup list --host my-vps --app freshrss
```

### Output Formats

```bash
auberge backup list --format json
auberge backup list --format yaml
```

## Restoring Backups

### Same-Host Restore

Restore the latest backup to the same host:

```bash
auberge backup restore latest --host my-vps
```

Restore a specific backup by timestamp:

```bash
auberge backup restore 2026-01-23_14-30-00 --host my-vps
```

Restore only specific apps:

```bash
auberge backup restore latest --host my-vps --apps baikal,freshrss
```

Dry run to preview:

```bash
auberge backup restore latest --host my-vps --dry-run
```

### Cross-Host Restore (Migration)

The `--from-host` flag enables restoring backups from one host to a different host. This is useful for:

- **VPS Provider Migration**: Moving from one provider to another (e.g., IONOS → Hetzner)
- **Disaster Recovery**: Restoring to a fresh VPS after hardware failure
- **Dev/Test Environments**: Seeding staging with production-like data

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

## Cross-Host Restore Safety Features

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

## Best Practices

### Regular Backups

Set up a cron job for regular backups:

```bash
0 2 * * * auberge backup create --host my-vps --apps baikal,freshrss,navidrome
```

### Retention Policy

Keep multiple timestamped backups and periodically clean old ones:

```bash
# Keep last 7 days, delete older
find ~/.local/share/auberge/backups/my-vps/*/20* -type d -mtime +7 -exec rm -rf {} \;
```

### Test Restores

Periodically test restores to verify backups are working:

```bash
auberge backup restore latest --host my-vps --apps baikal --dry-run
```

### Before Major Changes

Always create a backup before running Ansible playbooks or system updates:

```bash
auberge backup create --host my-vps
auberge ansible run --host my-vps --tags baikal
```

## Troubleshooting

### "No backups found for host"

The backup directory doesn't exist for the specified host. Create a backup first:

```bash
auberge backup create --host my-vps
```

### "SSH key not found"

See [docs/ssh.md](./ssh.md) for SSH key configuration options.

### "Required service not found on target host"

The target host is missing required systemd services. Run Ansible to install:

```bash
auberge ansible run --host new-vps
```

### "Insufficient disk space"

Target host doesn't have enough space for the restore. Free up space or use a larger VPS.

### Backup/Restore Hangs

Check network connectivity and SSH connection:

```bash
ssh -v user@host
```

If using SSH ControlMaster, clean up stale sockets:

```bash
rm /tmp/ssh-*
```

### Service Won't Start After Restore

Check service logs:

```bash
ssh user@host 'journalctl -u php*-fpm -n 50'
```

Common issues:

- File permissions (automatically fixed during restore; if still broken, re-run Ansible)
- Missing dependencies (fix: reinstall service)
- Port conflicts (fix: check `netstat -tulpn`)

## OPML Export/Import (FreshRSS)

Export and import RSS feed subscriptions independently of full backups.

### Export Feeds

```bash
auberge backup export-opml --host my-vps --output feeds.opml
```

With custom FreshRSS user:

```bash
auberge backup export-opml --host my-vps --output feeds.opml --user admin
```

### Import Feeds

```bash
auberge backup import-opml --host my-vps --input feeds.opml
```

This uploads the OPML file to the server and imports it into FreshRSS, then cleans up the temporary file.

## Technical Details

### Backup Process

1. Services are stopped via `systemctl stop {service}`
2. Data is synced from remote using `rsync` with SSH
3. Services are restarted via `systemctl start {service}`
4. `latest` symlink is updated to point to new backup

### Restore Process

1. Pre-flight validation (cross-host only)
2. Emergency backup creation (cross-host only)
3. User confirmation (hostname typing for cross-host, Y/N for same-host)
4. Services are stopped on target
5. Data is synced to remote using `rsync` with SSH
6. File ownership is set to service user (e.g., `chown -R calibre:calibre /home/calibre`)
7. Services are restarted on target
8. Post-restore guidance displayed (cross-host only)

### Excluded Files

The following patterns are excluded from backups to reduce size:

- `.git/` directories (except Baikal's VCS)
- `node_modules/`
- `venv/`, `__pycache__/`
- `*.pyc`, `*.pyo`, `*.tmp`, `*.log`
- `.DS_Store`, `.cache/`
- `.Baikal.cache/`

### SSH Connection Pooling

Backup and restore operations use SSH ControlMaster for connection reuse:

- ControlPath: `/tmp/ssh-%r@%h:%p`
- ControlPersist: 60 seconds

This significantly speeds up operations that require multiple SSH commands.
