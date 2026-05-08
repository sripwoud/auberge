# Baikal

CalDAV/CardDAV server for calendar and contact sync, with automatic birthday calendar generation. Docs: [sabre.io/baikal](https://sabre.io/baikal)

- **URL**: `https://{baikal_subdomain}.{domain}`
- **Port**: internal (Caddy proxy)
- **Data**: `/opt/baikal/Specific/`

## Deploy

```bash
auberge deploy baikal
```

## Required config

| Key                     | Purpose                    |
| ----------------------- | -------------------------- |
| `baikal_subdomain`      | Subdomain for HTTPS access |
| `baikal_admin_password` | Admin password             |

## Notes

?> Complete initial setup at `https://{baikal_subdomain}.{domain}/admin/` after first deploy.

Endpoints: CalDAV and CardDAV both at `/dav.php`.

Birthday sync reads `BDAY` fields from contacts and populates a dedicated "Birthdays" calendar daily via systemd timer. Trigger manually:

```bash
sudo systemctl start baikal-birthday-sync.service
```

Backed up by default (`/opt/baikal/Specific`). See [Backup & Restore](../../backup-restore/overview.md).
