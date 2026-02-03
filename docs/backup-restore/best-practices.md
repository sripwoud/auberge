# Backup Best Practices

## Regular Backups

Set up a cron job for regular backups:

```bash
0 2 * * * auberge backup create --host my-vps --apps baikal,freshrss,navidrome
```

## Retention Policy

Keep multiple timestamped backups and periodically clean old ones:

```bash
# Keep last 7 days, delete older
find ~/.local/share/auberge/backups/my-vps/*/20* -type d -mtime +7 -exec rm -rf {} \;
```

## Test Restores

Periodically test restores to verify backups are working:

```bash
auberge backup restore latest --host my-vps --apps baikal --dry-run
```

## Before Major Changes

Always create a backup before running Ansible playbooks or system updates:

```bash
auberge backup create --host my-vps
auberge ansible run --host my-vps --tags baikal
```

## Music Files

By default, Navidrome backups exclude music files to save space. Only include music when:

- Migrating to a new VPS
- Your music library changed significantly
- You don't have another backup of your music

```bash
# Normal backup (excludes music)
auberge backup create --host my-vps

# Full backup including music
auberge backup create --host my-vps --include-music
```

## Off-Site Backups

The default backup location is `~/.local/share/auberge/backups/`. Consider:

- Syncing to external drive
- Cloud storage (encrypted)
- Network-attached storage

```bash
# Custom backup destination
auberge backup create --host my-vps --dest /mnt/external/backups
```

## SSH Key Management

Ensure your SSH keys are backed up separately. Auberge uses:

- `~/.config/auberge/hosts.toml` - Host configuration
- `~/.ssh/identities/` - SSH keys

Back up these directories along with application data.

## Related Documentation

- [Creating Backups](creating-backups.md) - Backup commands
- [Cross-Host Migration](cross-host-migration.md) - Disaster recovery
- [Overview](overview.md) - Technical details
