# Baikal

Modern CalDAV/CardDAV server for calendar and contact synchronization built on Sabre/DAV.

Official Documentation: [https://sabre.io/baikal](https://sabre.io/baikal)

## Deployment

```bash
auberge ansible run --tags baikal
```

## Configuration

Requires `baikal_subdomain` and `baikal_admin_password` set in `config.toml`.

The deployed `baikal.yaml` config file has mode `0640` (owner + group read) since it contains the `encryption_key` and `admin_passwordhash`.

Initial setup is done through the web interface at `https://{baikal_subdomain}.{domain}/admin/`.

## Features

- CalDAV for calendar synchronization
- CardDAV for contact synchronization
- Web-based administration interface
- SQLite database backend
- Standards-compliant implementation
- Low resource footprint
- **Automatic birthday calendar** - Syncs birthdays from contacts to a dedicated calendar

## Access

- Admin Interface: `https://{baikal_subdomain}.{domain}/admin/`
- CalDAV Endpoint: `https://{baikal_subdomain}.{domain}/dav.php`
- CardDAV Endpoint: `https://{baikal_subdomain}.{domain}/dav.php`

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
systemctl status baikal-birthday-sync.timer
journalctl -u baikal-birthday-sync.service -n 50
```

The birthday calendar will appear in your CalDAV client alongside your other calendars as "Birthdays".

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
