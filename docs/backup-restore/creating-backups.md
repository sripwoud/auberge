# Creating Backups

## Backup All Apps

```bash
auberge backup create --host my-vps
```

This creates a timestamped backup of all applications with their data and configuration.

## Backup Specific Apps

```bash
auberge backup create --host my-vps --apps baikal,freshrss
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

## Database Backups (Paperless-ngx)

Paperless-ngx uses PostgreSQL for metadata (tags, correspondents, document types, users, permissions). The backup process automatically runs `pg_dump -Fc` to create a compressed database dump alongside the file backup.

The dump is stored as `db.dump` within the app's backup directory:

```
backups/{host}/{timestamp}/paperless/
├── opt/paperless/data/...
├── opt/paperless/media/...
└── db.dump
```

No additional flags are needed — database backup is automatic when backing up Paperless-ngx.

## Offsite Push

After creating a local backup, push it to an offsite restic repository:

```bash
auberge backup push --host my-vps
```

See [CLI Reference: backup push](../cli-reference/backup/push.md) for details.

## Related Commands

- [backup list](listing-backups.md) - List available backups
- [backup restore](restoring-backups.md) - Restore from backup
- [CLI Reference: backup create](../cli-reference/backup/create.md) - Full command documentation
