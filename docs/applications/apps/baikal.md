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

A privacy-sanitized iCalendar feed of your busy intervals, served at `https://{baikal_subdomain}.{domain}/busy.ics`. A host-side script (`baikal-busy-sync.py`) runs on a systemd timer (~15 min), expands recurrence into a rolling `[now − 1d, now + 60d]` window, and writes a `busy.ics` containing only opaque `Busy` blocks — UTC start/end plus a hashed per-instance UID. It never emits titles, locations, guests, descriptions, or the source UID.

**The feed is the privacy boundary.** Sanitization happens on the VPS, so no personal event content — and no external calendar credential — ever leaves the host. The `TRANSP:TRANSPARENT` filter auto-excludes the generated "Birthdays" calendar and anything you mark free; `STATUS:CANCELLED` events are dropped too.

Trigger manually (a failed run shows red in `systemctl list-timers` and leaves the previous `busy.ics` intact):

```bash
sudo systemctl start baikal-busy-sync.service
```

### Sources

| Source       | State     | Notes                                                                                                                                                     |
| ------------ | --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Baikal       | always on | All calendars under your principal.                                                                                                                       |
| Apple iCloud | opt-in    | One read-only external CalDAV calendar, fetched on the VPS via `caldav` against `caldav.icloud.com`, sanitized identically and merged into the same feed. |

With no iCloud config the feed is Baikal-only. Fail-fast: if iCloud is configured but the fetch fails, the run fails (red timer) rather than silently emitting a Baikal-only feed that under-reports busy time.

### Config

| Key                               | Secret | Required | Purpose                                                                                                                                                  |
| --------------------------------- | ------ | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `baikal_busy_feed_token`          | yes    | yes      | Token for the `X-Auberge-Busy-Token` header that gates `/busy.ics`.                                                                                      |
| `baikal_busy_icloud_username`     | no     | no       | Apple ID email for iCloud CalDAV.                                                                                                                        |
| `baikal_busy_icloud_app_password` | yes    | no       | Apple app-specific password (appleid.apple.com → Sign-In and Security → App-Specific Passwords). Lives only on the VPS; never sent to Google.            |
| `baikal_busy_icloud_calendar`     | no     | no       | Which iCloud calendar to merge: its display name (Apple Calendar / Thunderbird) or a full CalDAV URL (Thunderbird → Properties → Location). Empty = all. |

iCloud is enabled only when `baikal_busy_icloud_username` and `baikal_busy_icloud_app_password` are both set.

### Verify the feed

```bash
curl -H "X-Auberge-Busy-Token: <token>" https://{baikal_subdomain}.{domain}/busy.ics
```

?> The token goes in a header, never the URL or query string, so it stays out of Caddy's access log — Caddy's log filter strips that header. A request with no header, or a wrong token, returns `403`.

### Mirror availability to Google Calendar

auberge ships no consumer. `examples/baikal-busy-blockers.gs` is a reference, unsupported, operator-owned Google Apps Script that fetches the feed and upserts private `Busy` blocker events on your org **primary** calendar — so coworkers' "Find a time" sees you as busy. It diffs by a hashed-UID tag, so re-runs are idempotent.

1. [script.google.com](https://script.google.com) → New project → paste the contents of `examples/baikal-busy-blockers.gs`.
2. Project Settings → Script Properties → add `FEED_URL` = `https://{baikal_subdomain}.{domain}/busy.ics`, `FEED_TOKEN` = your `baikal_busy_feed_token`, optionally `CALENDAR_ID` (default `primary`) and `WINDOW_DAYS` (default `60`).
3. Run `syncBusyBlockers` once and approve the OAuth consent — you are authorizing your own script in your own account, so no org admin approval is needed. On the "unverified app" warning: Advanced → Go to project → Allow.
4. Add a time-driven trigger: Triggers (clock icon) → Add Trigger → `syncBusyBlockers`, Time-driven, Minutes timer, Every 10 minutes.

?> Do not click **Deploy** — that is for web apps/APIs. Time-driven triggers run the saved code directly, on Google's servers, even when your laptop is off.

End-to-end lag is the host timer (~15 min) plus the Apps Script trigger (~10 min) — up to ~25 min.

### Rotate the token or iCloud password

Feed token:

1. Update `baikal_busy_feed_token` in `config.toml`.
2. `auberge deploy baikal` (re-renders the Caddy route).
3. Update `FEED_TOKEN` in the Apps Script Script Properties.

iCloud app password:

1. Regenerate at appleid.apple.com.
2. Update `baikal_busy_icloud_app_password` in `config.toml`.
3. `auberge deploy baikal`.

The design rationale — host-sanitized pull feed vs OAuth push vs `VFREEBUSY`, and the optional external CalDAV source — is recorded in [ADR-0010](https://github.com/sripwoud/auberge/blob/master/meta/adr/0010-baikal-busy-feed-host-sanitized-external-consumer.md).
