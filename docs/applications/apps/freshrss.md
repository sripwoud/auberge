# FreshRSS

Self-hosted RSS feed aggregator and reader.

Official Documentation: [https://freshrss.org](https://freshrss.org)

## Deployment

```bash
auberge ansible run --tags freshrss
```

## Feed Updates

Feeds are automatically refreshed every 15 minutes via a systemd timer (`freshrss-update.timer`).

```bash
sudo systemctl status freshrss-update.timer
sudo journalctl -u freshrss-update.service --since "1 hour ago"
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
