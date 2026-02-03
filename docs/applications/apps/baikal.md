# Baikal

Modern CalDAV/CardDAV server for calendar and contact synchronization built on Sabre/DAV.

Official Documentation: [https://sabre.io/baikal](https://sabre.io/baikal)

## Deployment

```bash
auberge ansible run --tags baikal
```

## Configuration

Requires `BAIKAL_SUBDOMAIN` and `BAIKAL_ADMIN_PASSWORD` environment variables. See [Environment Variables](../../configuration/environment-variables.md).

Initial setup is done through the web interface at `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/admin/`.

## Features

- CalDAV for calendar synchronization
- CardDAV for contact synchronization
- Web-based administration interface
- SQLite database backend
- Standards-compliant implementation
- Low resource footprint

## Access

- Admin Interface: `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/admin/`
- CalDAV Endpoint: `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/dav.php`
- CardDAV Endpoint: `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/dav.php`

## Backup

Backed up by default. Backs up the `/opt/baikal/Specific` directory containing all data and configuration.

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
