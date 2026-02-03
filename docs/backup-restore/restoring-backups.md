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

## Related Commands

- [backup create](creating-backups.md) - Create backups
- [backup list](listing-backups.md) - List available backups
- [Cross-Host Migration](cross-host-migration.md) - Restore between different hosts
- [CLI Reference: backup restore](../cli-reference/backup/restore.md) - Full command documentation
