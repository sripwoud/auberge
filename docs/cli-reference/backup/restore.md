# auberge backup restore

Restore application data from backup

## Synopsis

```bash
auberge backup restore [OPTIONS] <BACKUP_ID>
```

## Alias

`auberge b r`

## Description

Restores application data from a backup. Supports same-host restore and cross-host migration with safety checks.

**Critical safety features**:

- Pre-flight validation (SSH, services, disk space)
- Emergency backup before cross-host restore
- Interactive confirmation for destructive operations
- Automatic Ansible playbook run to fix permissions

## Arguments

| Argument  | Description                                        |
| --------- | -------------------------------------------------- |
| BACKUP_ID | Backup timestamp (YYYY-MM-DD_HH-MM-SS) or 'latest' |

## Options

| Option                 | Description                       | Default                         |
| ---------------------- | --------------------------------- | ------------------------------- |
| -H, --host HOST        | Target host                       | Interactive selection           |
| -F, --from-host HOST   | Source host (for migration)       | Same as target                  |
| -a, --apps APPS        | Apps to restore (comma-separated) | All in backup                   |
| -k, --ssh-key PATH     | SSH private key                   | ~/.ssh/identities/{user}_{host} |
| -n, --dry-run          | Preview without restoring         | false                           |
| -y, --yes              | Skip confirmation prompt          | false                           |
| --skip-playbook-unsafe | UNSAFE: Skip Ansible playbook     | false                           |

## Examples

```bash
# Restore latest backup (interactive)
auberge backup restore latest

# Restore specific timestamp
auberge backup restore 2024-01-27_14-30-00 --host myserver

# Restore specific apps only
auberge backup restore latest --host myserver --apps radicale,freshrss

# Cross-host migration
auberge backup restore latest --host newserver --from-host oldserver

# Dry run to preview
auberge backup restore latest --host myserver --dry-run

# Skip confirmation (for automation)
auberge backup restore latest --host myserver --yes
```

## Restore Process

### Same-Host Restore

1. Load backup from specified timestamp
2. Show restore plan
3. Prompt for confirmation
4. For each app:
   - Stop service
   - Restore files with rsync
   - Set ownership
   - Start service
5. Run Ansible playbooks to fix permissions

### Cross-Host Migration

1. **Pre-flight validation**:
   - Check SSH connectivity
   - Verify services exist on target
   - Check disk space (requires 120% of backup size)

2. **Create emergency backup**:
   - Backup current state of target host
   - Stores as `pre-migration-{timestamp}`

3. **Show migration plan**:
   - Source and target hosts
   - Apps to migrate
   - Confirmation required

4. **Restore data** (same as above)

5. **Post-restore verification**:
   - Manual checks required
   - Service status
   - Log inspection
   - DNS updates
   - SSL certificates

## Restore Plan Example

```
=== Restore Plan ===
Host: myserver
Backup ID: 2024-01-27_14-30-00

Apps to restore:
  - radicale     from /path/to/backup/radicale
  - freshrss     from /path/to/backup/freshrss
  - navidrome    from /path/to/backup/navidrome

⚠ WARNING: This will overwrite existing data on the remote host!
Continue with restore? [y/N]:
```

## Cross-Host Migration Example

```
=== Restore Plan ===
Source: oldserver (backup: latest)
Target: newserver (192.168.1.20:22)

⚠  CROSS-HOST RESTORE WARNING
   This will restore data from 'oldserver' to 'newserver'
   Existing data on 'newserver' will be OVERWRITTEN

--- Pre-flight Validation ---
  Checking SSH connectivity...
    ✓ SSH connection successful
  Checking services on target...
    ✓ radicale service exists
    ✓ freshrss service exists
  Checking disk space...
    Available: 45.2 GB, Required: 24.1 GB (with 20% buffer)
    ✓ Sufficient disk space available
✓ Pre-flight validation completed

--- Creating Emergency Backup ---
  Backing up current state of 'newserver' before cross-host restore
  ✓ Emergency backup created: pre-migration-2024-01-27_15-00-00
    Location: ~/.local/share/auberge/backups/newserver/pre-migration-2024-01-27_15-00-00/

⚠  DANGER: Cross-host restore requires explicit confirmation
   Type the target host name 'newserver' to confirm:
Target host name: newserver

[1/2] Starting restore...
[2/2] Running Ansible playbooks to fix permissions...
✓ Ansible playbooks completed successfully

=== Post-Restore Actions Required ===
  Cross-host restore completed. Manual verification needed:

  1. Verify services are running:
     ssh ansible@192.168.1.20 'systemctl status radicale freshrss'

  2. Check service logs for errors:
     ssh ansible@192.168.1.20 'journalctl -u radicale --since "5 minutes ago" | grep -i error'

  3. Update DNS records if hostnames changed

  4. Verify SSL certificates are valid for new domain
```

## Permission Fixing

After restore, Ansible playbooks automatically run to:

- Set correct file ownership
- Fix directory permissions
- Restart services

To skip (UNSAFE):

```bash
auberge backup restore latest --host myserver --skip-playbook-unsafe
```

If skipped, manually run:

```bash
cd ansible && ansible-playbook playbooks/apps.yml --tags radicale,freshrss
```

## Troubleshooting

**Services fail after restore**:

- Check file ownership: `ls -la /var/lib/radicale`
- Run playbooks manually: `ansible-playbook playbooks/apps.yml --tags radicale`

**Cross-host restore validation fails**:

- **SSH connectivity**: Check SSH key and firewall
- **Service not found**: Run `auberge ansible run` to install apps first
- **Insufficient disk space**: Free up space or exclude large apps

**Emergency backup failed**:

- Can continue without it (dangerous)
- Recommended: Fix issue and retry

## Related Commands

- [auberge backup create](create.md) - Create new backup
- [auberge backup list](list.md) - List available backups
- [auberge ansible run](../ansible/run.md) - Run playbooks manually

## See Also

- [Backup and Restore Guide](../../backup-restore/README.md)
- [Migration Guide](../../backup-restore/migration.md)
