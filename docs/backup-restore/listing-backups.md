# Listing Backups

## List All Backups

```bash
auberge backup list
```

Example output:

```
HOST            APP          TIMESTAMP            SIZE
-----------------------------------------------------------------
my-vps          radicale     2026-01-23_14-30-00  3.24 MB
my-vps          freshrss     2026-01-23_14-30-01  39.57 MB
my-vps          navidrome    2026-01-23_14-30-02  856.06 KB
```

## Filter by Host or App

```bash
auberge backup list --host my-vps
auberge backup list --app radicale
auberge backup list --host my-vps --app freshrss
```

## Output Formats

```bash
auberge backup list --format json
auberge backup list --format yaml
```

## Related Commands

- [backup create](creating-backups.md) - Create backups
- [backup restore](restoring-backups.md) - Restore from backup
- [CLI Reference: backup list](../cli-reference/backup/list.md) - Full command documentation
