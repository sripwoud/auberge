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

## Busy Feed

A privacy-sanitized iCalendar feed of your busy intervals, served at `https://{baikal_subdomain}.{domain}/busy.ics`. A host-side script (`baikal-busy-sync.py`) reads Baikal's calendar data on a 15-minute systemd timer, expands recurrence into a `[now − 1d, now + 60d]` window, and writes a `busy.ics` containing only opaque `Busy` blocks — UTC start/end plus a hashed per-instance UID. It never emits titles, locations, guests, descriptions, or the source UID.

**The feed is the privacy boundary.** Sanitization happens on the VPS, so no personal event content ever leaves the host. The `TRANSP:TRANSPARENT` filter also auto-excludes the generated "Birthdays" calendar.

Trigger manually:

```bash
sudo systemctl start baikal-busy-sync.service
```

### Required config

| Key                      | Purpose                                                                |
| ------------------------ | ---------------------------------------------------------------------- |
| `baikal_busy_feed_token` | Secret required in the `X-Auberge-Busy-Token` header to fetch the feed |

Verify the feed (the token goes in a header, never the URL, so it stays out of access logs):

```bash
curl -H "X-Auberge-Busy-Token: <token>" https://{baikal_subdomain}.{domain}/busy.ics
```

A request without the header — or with a wrong token — returns `403`.

### Mirror availability to Google Calendar

The token guards the busy/free pattern from scrapers; it protects no event detail, so it is safe to store in a consumer that lives in your Google account. `examples/baikal-busy-blockers.gs` is a reference Google Apps Script (unsupported, operator-owned) that fetches the feed on a ~10-minute trigger and upserts private `Busy` blocker events on your org **primary** calendar, so coworkers' "Find a time" sees you as busy. Setup instructions are in the script header.

### Token rotation

1. Update `baikal_busy_feed_token` in your `config.toml`.
2. Redeploy: `auberge deploy baikal` (re-renders the Caddy route).
3. Update `FEED_TOKEN` in the consumer's Script Properties.

The design rationale — why a host-sanitized pull feed instead of an OAuth push or a `VFREEBUSY` component — is recorded in [ADR-0010](https://github.com/sripwoud/auberge/blob/master/meta/adr/0010-baikal-busy-feed-host-sanitized-external-consumer.md).
