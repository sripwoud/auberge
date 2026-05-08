# WebDAV

File sharing and sync via WebDAV protocol. Repo: [github.com/hacdias/webdav](https://github.com/hacdias/webdav)

- **URL**: `https://{subdomain}.{domain}`
- **Data**: shared files on VPS

## Deploy

```bash
auberge deploy webdav
```

## Required config

| Key               | Purpose                          |
| ----------------- | -------------------------------- |
| `WEBDAV_PASSWORD` | WebDAV access password (env var) |

## Notes

Backed up by default (all shared files). See [Backup & Restore](../../backup-restore/overview.md).
