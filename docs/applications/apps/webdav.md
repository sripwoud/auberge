# WebDAV

File sharing and synchronization via WebDAV protocol.

Official Repository: [https://github.com/hacdias/webdav](https://github.com/hacdias/webdav)

## Deployment

```bash
auberge ansible run --tags webdav
```

## Configuration

Requires `WEBDAV_PASSWORD` environment variable. See [Environment Variables](../../configuration/environment-variables.md).

## Backup

Backed up by default. Includes all shared files.

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
