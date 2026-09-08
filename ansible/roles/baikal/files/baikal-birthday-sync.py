#!/usr/bin/env python3

import re
import sys
from datetime import datetime, timedelta, timezone

from baikal_sync import (
    MAX_INT32,
    CalendarObject,
    CalendarSync,
    as_text,
    extract_name,
    operator_principal,
    render,
    unfold,
)

ANCHOR_YEAR = 1972
BDAY_LINE = re.compile(r"BDAY(?:;[^:]*)?:([^\r\n]+)")
YEAR_OMITTED_DATE = re.compile(r"--(\d{2})-?(\d{2})")


class BaikalBirthdaySync(CalendarSync):
    uid_prefix = "baikal-birthday-"
    calendar_uri = "birthdays"
    calendar_name = "Birthdays"
    calendar_description = "Auto-generated birthday calendar"

    def _parse_bday(self, vcard_data):
        if not vcard_data:
            return None
        match = BDAY_LINE.search(vcard_data)
        if not match:
            return None
        value = match.group(1).strip()

        if value.startswith("--"):
            reduced = YEAR_OMITTED_DATE.fullmatch(value)
            if not reduced:
                return None
            month, day = int(reduced.group(1)), int(reduced.group(2))
            try:
                datetime(ANCHOR_YEAR, month, day)
            except ValueError:
                return None
            return month, day, None

        if "-" in value:
            try:
                dt = datetime.strptime(value[:10], "%Y-%m-%d")
                return dt.month, dt.day, dt.year
            except ValueError:
                return None

        if len(value) == 8:
            try:
                dt = datetime.strptime(value, "%Y%m%d")
                return dt.month, dt.day, dt.year
            except ValueError:
                return None

        return None

    def _build_vevent(self, uid, name, month, day, year):
        start_year = year or ANCHOR_YEAR
        start = f"{start_year:04d}{month:02d}{day:02d}"
        end_dt = datetime(start_year, month, day) + timedelta(days=1)
        end = end_dt.strftime("%Y%m%d")
        now = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

        return render(
            [
                "BEGIN:VCALENDAR",
                "VERSION:2.0",
                "PRODID:-//Baikal//Birthday Sync//EN",
                "BEGIN:VEVENT",
                f"UID:{uid}@baikal-birthday",
                f"DTSTAMP:{now}",
                f"CREATED:{now}",
                f"LAST-MODIFIED:{now}",
                f"DTSTART;VALUE=DATE:{start}",
                f"DTEND;VALUE=DATE:{end}",
                f"SUMMARY:{name}'s Birthday",
                "RRULE:FREQ=YEARLY",
                "TRANSP:TRANSPARENT",
                "CLASS:PUBLIC",
                "CATEGORIES:Birthday",
                "BEGIN:VALARM",
                "ACTION:DISPLAY",
                "DESCRIPTION:Birthday reminder",
                "TRIGGER:-PT4H",
                "END:VALARM",
                "END:VEVENT",
                "END:VCALENDAR",
            ]
        )

    def _first_occurrence_ts(self, month, day, year):
        return int(datetime(year or ANCHOR_YEAR, month, day, tzinfo=timezone.utc).timestamp())

    def sync_birthdays(self):
        self.connect()
        cursor = self.conn.cursor()

        principal_uri = operator_principal(cursor)
        if not principal_uri:
            print("No operator principal found", file=sys.stderr)
            return False

        self._get_or_create_calendar(principal_uri)

        cursor.execute("SELECT carddata, uri FROM cards")
        contacts = cursor.fetchall()
        if not contacts:
            print("No contacts found")
            return True

        unparsable = 0

        def objects():
            nonlocal unparsable
            for contact in contacts:
                carddata = unfold(as_text(contact["carddata"]) or "")
                bday = self._parse_bday(carddata)
                if not bday:
                    if carddata and BDAY_LINE.search(carddata):
                        unparsable += 1
                    continue

                month, day, year = bday
                uid = self._make_uid(contact["uri"])
                yield CalendarObject(
                    uri=uid + ".ics",
                    ical=self._build_vevent(uid, extract_name(carddata), month, day, year),
                    first_occurrence=self._first_occurrence_ts(month, day, year),
                    last_occurrence=MAX_INT32,
                )

        count = self.write_events(cursor, objects())

        self.conn.commit()
        if unparsable:
            print(f"Unparsable BDAY values: {unparsable}", file=sys.stderr)
        print(f"Synced {count} birthdays")
        return True


def main():
    db_path = "/opt/baikal/Specific/db/db.sqlite"
    if len(sys.argv) > 1:
        db_path = sys.argv[1]

    sync = BaikalBirthdaySync(db_path)
    try:
        sys.exit(0 if sync.sync_birthdays() else 1)
    finally:
        sync.close()


if __name__ == "__main__":
    main()
