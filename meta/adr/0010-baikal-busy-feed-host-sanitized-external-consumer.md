# ADR-0010: Baikal Busy Feed — host-sanitized availability, consumed by an external script

## Status

Accepted, 2026-06-09.

## Decision

A host-side script (`baikal-busy-sync.py`) on the baikal Host, driven by a systemd timer, reads Baikal's SQLite `calendarobjects`, filters to busy events (skip `STATUS:CANCELLED` and `TRANSP:TRANSPARENT`), expands recurrence (`icalendar` + `recurring-ical-events`) into a `[now − 1d, now + 60d]` rolling window, and emits a **Busy Feed**: a sanitized `busy.ics` containing only opaque `Busy` `VEVENT`s (UTC start/end + a `sha256`-hashed per-instance UID) — never titles, locations, guests, descriptions, or the source UID. Caddy serves it on Baikal's existing public site at `/busy.ics`, gated by a request header matching `baikal_busy_feed_token` (a new secret Key Registry key); the file lives outside `html/` so it is never reachable unguarded.

auberge ships no consumer. `examples/baikal-busy-blockers.gs` is a reference Google Apps Script (headed "REFERENCE — NOT A SUPPORTED TOOL", like `bichon-expunge.sh`) that, on a time-driven trigger, fetches the feed and upserts private `Busy` blocker events on the operator's org **primary** calendar, diffing by the hashed UID stored via `setTag`. No new Rust.

## Why

Operator priorities, in order:

1. **Personal event detail must never reach the org Google account.** Sanitization happens on the VPS, so the Host — not any Google account, and not Google's free/busy sharing — is the privacy boundary. Even a Workspace admin with elevated access finds only `Busy` times (visible by design anyway), never appointment contents, because the contents are not in Google at all.
2. **Coworkers' "Find a time" must see the operator as busy.** Only events on the org _primary_ calendar surface to other users' availability views; subscribed/secondary calendars do not. Hence real blocker events, written via the consumer.
3. **Timely.** Blockers written via the API affect availability instantly, avoiding Google's uncontrollable "From URL" refresh latency.

A **pull from a public feed is forced by rejecting OAuth**: a _push_ from auberge into Google needs the third-party OAuth grant the org will not approve. The token only keeps internet scrapers off the busy/free pattern; it guards no detail, so the token residing in the org account (where an admin could read it) is a non-event.

## Considered alternatives

- **External OAuth app writing the org calendar directly.** Rejected: the org (Exodus) will not approve a third-party app writing calendars, and unverified-app refresh tokens expire every 7 days — unworkable operationally.
- **Google "From URL" ICS subscription.** Rejected: Google re-fetches on its own schedule (hours up to ~a day) and ignores refresh hints, so same-day appointments do not block; and external subscribed calendars do not surface to coworkers' "Find a time."
- **Personal-Google intermediary (`caldav → personal Google → org Google`).** Rejected: re-centralizes full calendar detail _into_ Google, defeating the reason Baikal is self-hosted; still requires the org-side blocker script (a shared calendar is secondary, invisible to Find-a-time); and adds an account, a cross-account share, a second sync hop, and a new org-policy dependency (external calendar sharing may be blocked just as OAuth is).
- **Apps Script reads Baikal CalDAV directly.** Rejected: `UrlFetchApp` cannot perform Baikal's Digest auth (no nonce challenge-response); switching Baikal to Basic weakens auth for all clients and still routes full event detail through Google's script execution and logs.
- **`VFREEBUSY` component, or a bespoke JSON feed.** Rejected: `VFREEBUSY` periods carry no per-instance UID, so the consumer cannot address individual blockers to update/delete them; JSON breaks the tool-agnostic-artifact principle (ADR-0006) — discrete `VEVENT`s let any future consumer subscribe to the same feed.
- **New Rust subcommand in auberge.** Rejected on ADR-0006's reasoning: a host-side Python script + systemd timer + Caddy route is sufficient; there is no binary to ship.

## Consequences

**Positive:**

- Personal event detail never leaves the VPS — no Google account, including the org's, ever holds it. The org admin's reach is bounded to `Busy` times.
- Blockers on the org primary calendar surface to coworkers instantly.
- No auberge runtime dependency on Google, and no new Rust; the feed is observable (`curl -H X-Auberge-Busy-Token:… …/busy.ics`).
- Birthdays are auto-excluded: the generated `birthdays` calendar is `TRANSP:TRANSPARENT`, which the busy filter drops without a special case.

**Negative:**

- End-to-end lag is the sum of the host timer (~15 min) and the Apps Script trigger (~10 min).
- A wedged sync timer leaves the feed stale; surface it via `systemctl list-timers` / journal as with bichon's archive timer.
- The consumer is unsupported, untested reference code the operator owns and adapts.
- The token lives in the org account (acceptable — it guards no detail).
- Adds `icalendar` + `recurring-ical-events` in a venv on the baikal Host (the existing `baikal-birthday-sync.py` is stdlib-only); justified by correct timezone/recurrence/override expansion, where a silent miss means a double-booking.

## Amendment (2026-06-10): optional external CalDAV source

`baikal-busy-sync.py` may merge one additional source: a read-only external CalDAV calendar (the operator's case: an Apple iCloud shared calendar). The Host fetches it over CalDAV (`caldav` library against `caldav.icloud.com`), runs it through the **same** `recurring-ical-events` expansion + `STATUS`/`TRANSP` filter + hashed-UID sanitization as Baikal's calendars, and merges it into the one `busy.ics`. iCloud objects are raw ICS strings shaped like Baikal's `calendarobjects.calendardata`, so the sanitize pipeline is unchanged — only the source list grows.

This does not move the privacy boundary: external event detail is sanitized on the Host exactly like Baikal's, and the external CalDAV credential (an Apple app-specific password) is resident only on the VPS — delivered via a `0600` `EnvironmentFile` (the bichon-archive pattern), never argv, never reaching Google. The source is opt-in: with no iCloud config the feed is Baikal-only, as before. Per the project's fail-fast rule, a configured-but-failing iCloud fetch exits non-zero (red timer) and leaves the prior `busy.ics` intact, rather than silently emitting a Baikal-only feed that would under-report busy time.

Adds `caldav` to the busy-feed venv, justified the same way as `recurring-ical-events`: a silent miss is a double-booking. New optional Key Registry keys: `baikal_busy_icloud_username`, `baikal_busy_icloud_app_password` (secret), `baikal_busy_icloud_calendar` (display name or CalDAV URL; empty = all calendars under the account).

## References

- ADR-0006 — Bichon archive feeds the Backup Recipe. Same shape: a host-side script produces a tool-agnostic artifact on a timer; auberge ships no consumer and no Rust.
- ADR-0005 — Substrate Apps in `infrastructure.yml`. Same philosophy: host-side state runs reliably without operator-laptop coordination.
- CONTEXT.md — **Busy Feed**.
