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
- **Automatic birthday calendar** - Syncs birthdays from contacts to a dedicated calendar

## Access

- Admin Interface: `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/admin/`
- CalDAV Endpoint: `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/dav.php`
- CardDAV Endpoint: `https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/dav.php`

## Backup

Backed up by default. Backs up the `/opt/baikal/Specific` directory containing all data and configuration.

See [Backup & Restore](../../backup-restore/overview.md).

## Birthday Calendar

Auberge automatically deploys a birthday calendar sync system that:

- Reads birthday information from your CardDAV contacts (BDAY field)
- Creates a dedicated "Birthdays" calendar with recurring annual events
- Automatically syncs daily via systemd timer
- Supports both full dates (with year) and year-less birthdays

The sync runs automatically once per day. To manually trigger a sync:

```bash
ssh your-vps
sudo systemctl start baikal-birthday-sync.service
```

To check the sync status:

```bash
# Check timer status
systemctl status baikal-birthday-sync.timer

# Check last sync run
journalctl -u baikal-birthday-sync.service -n 50
```

The birthday calendar will appear in your CalDAV client alongside your other calendars as "Birthdays".

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
