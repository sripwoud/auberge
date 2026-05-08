# FreshRSS

Self-hosted RSS feed aggregator and reader. Docs: [freshrss.org](https://freshrss.org)

- **URL**: `https://{subdomain}.{domain}`
- **Data**: SQLite database + user data on VPS

## Deploy

```bash
auberge deploy freshrss
```

## Notes

Feeds refresh every 15 minutes via `freshrss-update.timer`.

Backed up by default (SQLite DB, config, user data). Supports OPML export/import — see [OPML Management](../../backup-restore/opml-management.md).
