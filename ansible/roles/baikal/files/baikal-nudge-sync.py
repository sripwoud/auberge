#!/usr/bin/env python3

"""Generate a reach-out nudge for every contact carrying a cadence token.

The token in a contact's vCard `NOTE` is the whole opt-in: no token, no event,
so the dentists and landlords stay silent without needing a list to maintain.
"""

import re
import sys
from datetime import datetime, timedelta, timezone

from baikal_sync import (
    CalendarObject,
    CalendarSync,
    as_text,
    escape_text,
    extract_name,
    operator_principal,
    render,
    unescape_text,
    unfold,
)

DEFAULT_DB_PATH = "/opt/baikal/Specific/db/db.sqlite"
DEFAULT_CADENCE_DAYS = 90

NOTE_LINE = re.compile(r"NOTE(?:;[^:]*)?:([^\r\n]+)")

# `@d` takes the role default; `@30d` overrides it. The lookbehind is what keeps
# an email address out of the grammar: in `anna@gmail.com` the `@` follows a
# word character, so it can never open a token -- and neither can the `@` in
# `anna@d.example`, which a bare `@(\d*)d\b` would match.
CADENCE_TOKEN = re.compile(r"(?<![\w.@])@(\d*)d\b")

# The same token with the horizontal space either side of it, for removal. A
# token between two words closes to one space; one at either end closes to
# nothing. Collapsing all runs of whitespace instead would flatten a note's own
# indentation, which is the operator's, not an artifact of the token.
CADENCE_TOKEN_WITH_PADDING = re.compile(r"([ \t]*)(?<![\w.@])@\d*d\b([ \t]*)")


class BaikalNudgeSync(CalendarSync):
    uid_prefix = "baikal-nudge-"
    calendar_uri = "care"
    calendar_name = "Keep in touch"
    calendar_description = "Auto-generated reach-out nudges"

    def __init__(self, db_path, default_days=DEFAULT_CADENCE_DAYS):
        super().__init__(db_path)
        self.default_days = default_days

    def _cadence_days(self, note):
        """The cadence a note asks for, and how many tokens it carries.

        A note with two tokens takes the first: a contact must not quietly nag
        on a cadence you thought you had changed, so the count is reported.
        """
        matches = CADENCE_TOKEN.findall(note)
        if not matches:
            return None, 0
        return int(matches[0]) if matches[0] else self.default_days, len(matches)

    def _body(self, note):
        """The note as an RFC 5545 TEXT value, with every token removed.

        vCard stores `NOTE` already escaped in the same grammar iCalendar uses,
        so the value is unescaped before being re-escaped -- escaping the raw
        value would double every sequence and show the reader a literal `\\,`.
        """
        def close_the_gap(match):
            return " " if match.group(1) and match.group(2) else ""

        stripped = CADENCE_TOKEN_WITH_PADDING.sub(close_the_gap, unescape_text(note))
        return escape_text(stripped.strip())

    def _build_vevent(self, uid, name, start, body):
        end = start + timedelta(days=1)
        now = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

        return render(
            [
                "BEGIN:VCALENDAR",
                "VERSION:2.0",
                "PRODID:-//Baikal//Nudge Sync//EN",
                "BEGIN:VEVENT",
                f"UID:{uid}@baikal-nudge",
                f"DTSTAMP:{now}",
                f"CREATED:{now}",
                f"LAST-MODIFIED:{now}",
                f"DTSTART;VALUE=DATE:{start.strftime('%Y%m%d')}",
                f"DTEND;VALUE=DATE:{end.strftime('%Y%m%d')}",
                f"SUMMARY:Reach out: {escape_text(unescape_text(name))}",
                f"DESCRIPTION:{body}",
                "CATEGORIES:Nudge",
                "CLASS:PRIVATE",
                # Load-bearing, not cosmetic: baikal-busy-sync.py reads every
                # calendar under the principal with no uri filter, so `care` is
                # swept into the published free/busy feed and this line is the
                # only thing excluding it. Without it, an overdue contact
                # publishes an all-day busy block to anyone holding the feed
                # token -- which, under clamping, is most days.
                "TRANSP:TRANSPARENT",
                "END:VEVENT",
                "END:VCALENDAR",
            ]
        )

    def sync_nudges(self):
        self.connect()
        cursor = self.conn.cursor()

        principal_uri = operator_principal(cursor)
        if not principal_uri:
            print("No operator principal found", file=sys.stderr)
            return False

        self._get_or_create_calendar(principal_uri)

        cursor.execute("SELECT uri, carddata, lastmodified FROM cards")
        contacts = cursor.fetchall()

        today = datetime.now(timezone.utc).date()
        counts = {"opted_in": 0, "overdue": 0, "no_clock": 0, "multi_token": 0}

        def objects():
            for contact in contacts:
                carddata = unfold(as_text(contact["carddata"]) or "")
                if not carddata:
                    continue
                note_match = NOTE_LINE.search(carddata)
                if not note_match:
                    continue

                note = note_match.group(1).strip()
                days, tokens = self._cadence_days(note)
                if days is None:
                    continue

                counts["opted_in"] += 1
                if tokens > 1:
                    counts["multi_token"] += 1

                # `lastmodified` is nullable. A card with no clock is skipped
                # rather than read as epoch, which would leave it permanently
                # overdue with no edit able to satisfy it.
                clock = contact["lastmodified"]
                if clock is None:
                    counts["no_clock"] += 1
                    continue

                due = datetime.fromtimestamp(clock, timezone.utc).date() + timedelta(days=days)
                if due < today:
                    counts["overdue"] += 1

                # Overdue contacts clamp to today and resurface every run until
                # the card is touched. Writing the true past date would bury the
                # event where it is never seen -- for exactly the people this
                # exists for.
                start = max(due, today)
                uid = self._make_uid(contact["uri"])
                yield CalendarObject(
                    uri=uid + ".ics",
                    ical=self._build_vevent(uid, extract_name(carddata), start, self._body(note)),
                    first_occurrence=int(datetime.combine(start, datetime.min.time(), timezone.utc).timestamp()),
                    last_occurrence=int(
                        datetime.combine(start + timedelta(days=1), datetime.min.time(), timezone.utc).timestamp()
                    ),
                )

        written = self.write_events(cursor, objects())
        self.conn.commit()

        if counts["no_clock"]:
            print(f"Contacts opted in with no lastmodified: {counts['no_clock']}", file=sys.stderr)
        if counts["multi_token"]:
            print(f"Contacts with more than one cadence token: {counts['multi_token']}", file=sys.stderr)
        print(
            f"Synced {written} nudges from {counts['opted_in']} opted-in contacts ({counts['overdue']} overdue)"
        )
        return True


def main():
    db_path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DB_PATH
    default_days = int(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_CADENCE_DAYS

    sync = BaikalNudgeSync(db_path, default_days)
    try:
        sys.exit(0 if sync.sync_nudges() else 1)
    finally:
        sync.close()


if __name__ == "__main__":
    main()
