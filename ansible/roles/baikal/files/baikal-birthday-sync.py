#!/usr/bin/env python3

import hashlib
import re
import sqlite3
import sys
from datetime import datetime, timedelta, timezone

CRLF = "\r\n"
EPOCH_YEAR = 1970
MAX_INT32 = 2147483647
DYNAMIC_PREFIXES = ("DTSTAMP:", "CREATED:", "LAST-MODIFIED:")


def _stable_ical(ical_data):
    lines = ical_data.replace("\r\n", "\n").split("\n")
    return "\n".join(line for line in lines if not line.startswith(DYNAMIC_PREFIXES))


class BaikalBirthdaySync:
    def __init__(self, db_path):
        self.db_path = db_path
        self.conn = None
        self.calendar_id = None

    def connect(self):
        self.conn = sqlite3.connect(self.db_path, timeout=30.0)
        self.conn.execute("PRAGMA busy_timeout = 30000")
        self.conn.execute("PRAGMA journal_mode = WAL")
        self.conn.row_factory = sqlite3.Row

    def close(self):
        if self.conn:
            self.conn.close()

    def _parse_bday(self, vcard_data):
        if not vcard_data:
            return None
        match = re.search(r"BDAY(?:;[^:]*)?:([^\r\n]+)", vcard_data)
        if not match:
            return None
        value = match.group(1).strip()

        if value.startswith("--"):
            try:
                month, day = value[2:].split("-")
                return int(month), int(day), None
            except (ValueError, IndexError):
                return None

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

    def _extract_name(self, vcard_data):
        if not vcard_data:
            return "Unknown"
        match = re.search(r"FN:([^\r\n]+)", vcard_data)
        if match:
            return match.group(1).strip()
        match = re.search(r"N:([^;]+);([^;\r\n]*)", vcard_data)
        if match:
            return f"{match.group(2).strip()} {match.group(1).strip()}".strip()
        return "Unknown"

    def _make_uid(self, contact_uri):
        return hashlib.sha256(f"baikal-birthday-{contact_uri}".encode()).hexdigest()[:32]

    def _get_or_create_calendar(self, principal_uri):
        cursor = self.conn.cursor()
        cursor.execute(
            "SELECT calendarid FROM calendarinstances WHERE principaluri = ? AND uri = 'birthdays'",
            (principal_uri,),
        )
        row = cursor.fetchone()
        if row:
            self.calendar_id = row["calendarid"]
            return

        cursor.execute("INSERT INTO calendars (synctoken, components) VALUES (1, 'VEVENT')")
        self.calendar_id = cursor.lastrowid
        cursor.execute(
            """INSERT INTO calendarinstances
            (calendarid, principaluri, access, displayname, uri, description, transparent)
            VALUES (?, ?, 1, 'Birthdays', 'birthdays', 'Auto-generated birthday calendar', 0)""",
            (self.calendar_id, principal_uri),
        )
        self.conn.commit()

    def _bump_synctoken(self, cursor, uri, operation):
        cursor.execute("SELECT synctoken FROM calendars WHERE id = ?", (self.calendar_id,))
        new_token = (cursor.fetchone()["synctoken"] or 0) + 1
        cursor.execute(
            "INSERT INTO calendarchanges (uri, synctoken, calendarid, operation) VALUES (?, ?, ?, ?)",
            (uri, new_token, self.calendar_id, operation),
        )
        cursor.execute(
            "UPDATE calendars SET synctoken = ? WHERE id = ?",
            (new_token, self.calendar_id),
        )

    def _build_vevent(self, uid, name, month, day, year):
        start_year = year or EPOCH_YEAR
        start = f"{start_year:04d}{month:02d}{day:02d}"
        end_dt = datetime(start_year, month, day) + timedelta(days=1)
        end = end_dt.strftime("%Y%m%d")
        now = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

        lines = [
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
        return CRLF.join(lines) + CRLF

    def _first_occurrence_ts(self, month, day, year):
        return int(datetime(year or EPOCH_YEAR, month, day, tzinfo=timezone.utc).timestamp())

    def sync_birthdays(self):
        self.connect()
        cursor = self.conn.cursor()

        cursor.execute("SELECT uri FROM principals WHERE uri LIKE 'principals/%'")
        principals = cursor.fetchall()
        if not principals:
            print("No principals found", file=sys.stderr)
            return False

        principal_uri = principals[0]["uri"]
        self._get_or_create_calendar(principal_uri)

        cursor.execute("SELECT carddata, uri FROM cards WHERE carddata LIKE '%BDAY%'")
        contacts = cursor.fetchall()
        if not contacts:
            print("No contacts with birthdays found")
            return True

        cursor.execute(
            "SELECT uri, calendardata FROM calendarobjects WHERE calendarid = ?",
            (self.calendar_id,),
        )
        existing = {row["uri"]: row["calendardata"] for row in cursor.fetchall()}

        count = 0
        processed_uris = set()

        for contact in contacts:
            bday = self._parse_bday(contact["carddata"])
            if not bday:
                continue

            month, day, year = bday
            name = self._extract_name(contact["carddata"])
            uid = self._make_uid(contact["uri"])
            event_uri = uid + ".ics"
            processed_uris.add(event_uri)

            ical_data = self._build_vevent(uid, name, month, day, year)
            first_occ = self._first_occurrence_ts(month, day, year)
            now_ts = int(datetime.now(timezone.utc).timestamp())
            etag = hashlib.sha256(ical_data.encode("utf-8")).hexdigest()
            size = len(ical_data.encode("utf-8"))

            if event_uri in existing:
                if _stable_ical(existing[event_uri]) != _stable_ical(ical_data):
                    cursor.execute(
                        """UPDATE calendarobjects
                        SET calendardata = ?, lastmodified = ?, etag = ?, size = ?, uid = ?
                        WHERE calendarid = ? AND uri = ?""",
                        (ical_data, now_ts, etag, size, uid, self.calendar_id, event_uri),
                    )
                    self._bump_synctoken(cursor, event_uri, 2)
            else:
                cursor.execute(
                    """INSERT INTO calendarobjects
                    (calendarid, uri, calendardata, lastmodified, etag, size, componenttype, firstoccurence, lastoccurence, uid)
                    VALUES (?, ?, ?, ?, ?, ?, 'VEVENT', ?, ?, ?)""",
                    (self.calendar_id, event_uri, ical_data, now_ts, etag, size, first_occ, MAX_INT32, uid),
                )
                self._bump_synctoken(cursor, event_uri, 1)

            count += 1

        for stale_uri in set(existing.keys()) - processed_uris:
            cursor.execute(
                "DELETE FROM calendarobjects WHERE calendarid = ? AND uri = ?",
                (self.calendar_id, stale_uri),
            )
            self._bump_synctoken(cursor, stale_uri, 3)

        self.conn.commit()
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
