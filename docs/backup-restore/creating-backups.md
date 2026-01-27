# Creating Backups

## Backup All Apps

```bash
auberge backup create --host my-vps
```

This creates a timestamped backup of all applications with their data and configuration.

## Backup Specific Apps

```bash
auberge backup create --host my-vps --apps radicale,freshrss
```

## Include Music Files

By default, Navidrome backups exclude the music library (`/srv/music`) to save space and time. To include music files:

```bash
auberge backup create --host my-vps --include-music
```

## Dry Run

Preview what will be backed up without making changes:

```bash
auberge backup create --host my-vps --dry-run
```

## Custom Destination

```bash
auberge backup create --host my-vps --dest /mnt/external/backups
```

## Related Commands

- [backup list](listing-backups.md) - List available backups
- [backup restore](restoring-backups.md) - Restore from backup
- [CLI Reference: backup create](../cli-reference/backup/create.md) - Full command documentation
