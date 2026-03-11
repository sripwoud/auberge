# vdirsyncer

Two-way iCloud-to-Baikal calendar synchronization via CalDAV.

Official Documentation: [https://github.com/pimutils/vdirsyncer](https://github.com/pimutils/vdirsyncer)

## Prerequisites

- [Baikal](baikal.md) deployed and accessible
- A calendar created in the Baikal admin interface (`https://{BAIKAL_SUBDOMAIN}.{PRIMARY_DOMAIN}/admin/`)
- An iCloud app-specific password ([Apple instructions](https://support.apple.com/en-us/102654))
- Your iCloud CalDAV URL and calendar ID

## Configuration

```bash
auberge config set vdirsyncer_icloud_url <icloud-caldav-url>
auberge config set vdirsyncer_icloud_username <apple-id-email>
auberge config set vdirsyncer_icloud_password <app-specific-password>
auberge config set vdirsyncer_icloud_calendar_id <icloud-calendar-id>
auberge config set vdirsyncer_baikal_calendar_name <baikal-calendar-name>
```

The role is skipped entirely if `vdirsyncer_icloud_password` is not set, so vdirsyncer is opt-in by default.

## Deployment

```bash
auberge ansible run --tags vdirsyncer
```

## How It Works

A systemd timer triggers `vdirsyncer sync` every 2 minutes:

1. `vdirsyncer.timer` fires on schedule
2. `vdirsyncer.service` runs a one-shot sync
3. vdirsyncer pulls changes from iCloud CalDAV and pushes to Baikal (and vice versa)
4. On conflict, iCloud wins (`conflict_resolution = "a wins"`)

Sync state is persisted in `/var/lib/vdirsyncer/status`.

## Monitoring

```bash
systemctl status vdirsyncer.timer
journalctl -u vdirsyncer.service -n 50
```

## Backup

Backed up by default. Backs up the `/var/lib/vdirsyncer` directory containing sync state. Both `vdirsyncer.timer` and `vdirsyncer.service` are stopped during backup to prevent race conditions.

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Baikal](baikal.md)
- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
