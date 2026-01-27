# Radicale

Lightweight CalDAV/CardDAV server for calendar and contact synchronization.

Official Documentation: [https://radicale.org](https://radicale.org)

## Deployment

```bash
auberge ansible run --tags radicale
```

## Configuration

Requires `RADICALE_PASSWORD` environment variable. See [Environment Variables](../../configuration/environment-variables.md).

## Backup

Backed up by default. See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
