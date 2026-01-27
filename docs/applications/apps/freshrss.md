# FreshRSS

Self-hosted RSS feed aggregator and reader.

Official Documentation: [https://freshrss.org](https://freshrss.org)

## Deployment

```bash
auberge ansible run --tags freshrss
```

## Backup

Backed up by default. Includes:

- SQLite database
- Configuration
- User data

See [Backup & Restore](../../backup-restore/overview.md).

## OPML Export/Import

Supports independent feed export/import. See [OPML Management](../../backup-restore/opml-management.md).

## Related

- [Backup & Restore](../../backup-restore/overview.md)
- [OPML Management](../../backup-restore/opml-management.md)
- [Applications Overview](../overview.md)
