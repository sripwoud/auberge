# Restoring Backups

## Same-Host Restore

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

## Database Restore (Paperless-ngx)

When restoring Paperless-ngx, the process automatically:

1. Uploads the `db.dump` file to the remote host
2. Runs `pg_restore --clean --if-exists` to restore the PostgreSQL database
3. Executes Django migrations (`manage.py migrate`) to handle schema differences between backup and current Paperless version

If no `db.dump` is found in the backup, the restore continues with a warning (file data is still restored).

## Related Commands

- [backup create](creating-backups.md) - Create backups
- [backup list](listing-backups.md) - List available backups
- [Cross-Host Migration](cross-host-migration.md) - Restore between different hosts
- [CLI Reference: backup restore](../cli-reference/backup/restore.md) - Full command documentation
