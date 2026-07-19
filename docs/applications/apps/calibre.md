# Calibre

Calibre-Web ebook library with web reader. Lightweight alternative to [Grimmory](applications/apps/grimmory.md) (~50 MB RAM vs ~1 GB, no MariaDB or JVM). Docs: [calibre-ebook.com](https://calibre-ebook.com)

- **URL**: `https://{subdomain}.{domain}`
- **Data**: book library + metadata and user databases on VPS

## Deploy

```bash
auberge deploy calibre
```

## Notes

Backed up by default (book library, metadata DB, user DB). See [Backup & Restore](backup-restore/overview.md).
