#!/usr/bin/env python3

import hashlib
import os
import sqlite3
import sys
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import caldav
import icalendar
import recurring_ical_events

CRLF = "\r\n"
WINDOW_PAST_DAYS = 1
WINDOW_FUTURE_DAYS = 60
DEFAULT_DB_PATH = "/opt/baikal/Specific/db/db.sqlite"
DEFAULT_OUT_PATH = "/opt/baikal/busy/busy.ics"
ICLOUD_CALDAV_URL = "https://caldav.icloud.com"


@dataclass(frozen=True)
class BusyBlock:
    uid: str
    start: date | datetime
    end: date | datetime
    all_day: bool


@dataclass(frozen=True)
class IcloudConfig:
    username: str
    app_password: str
    calendar_ref: str


def _is_all_day(value):
    return isinstance(value, date) and not isinstance(value, datetime)


def _to_utc(value):
    if value.tzinfo is None:
        return value.replace(tzinfo=timezone.utc)
    return value.astimezone(timezone.utc)


def _instance_key(start):
    if _is_all_day(start):
        return start.isoformat()
    return _to_utc(start).isoformat()


def _make_uid(source_uid, start):
    return hashlib.sha256(f"{source_uid}-{_instance_key(start)}".encode()).hexdigest()[:32]


def _is_busy(component):
    if str(component.get("STATUS", "")).upper() == "CANCELLED":
        return False
    if str(component.get("TRANSP", "")).upper() == "TRANSPARENT":
        return False
    return True


def _icloud_calendar_data(config, window_start, window_end):
    client = caldav.DAVClient(url=ICLOUD_CALDAV_URL, username=config.username, password=config.app_password)
    ref = config.calendar_ref
    if ref.startswith("http"):
        calendars = [client.calendar(url=ref)]
    else:
        calendars = client.principal().calendars()
        if ref:
            calendars = [cal for cal in calendars if str(cal.name).casefold() == ref.casefold()]
            if not calendars:
                raise RuntimeError(f"iCloud calendar not found: {ref}")
    return [
        obj.data
        for cal in calendars
        for obj in cal.search(start=window_start, end=window_end, event=True, expand=False)
    ]


def _icloud_config_from_env():
    username = os.environ.get("BAIKAL_BUSY_ICLOUD_USERNAME", "").strip()
    app_password = os.environ.get("BAIKAL_BUSY_ICLOUD_APP_PASSWORD", "").strip()
    if not username or not app_password:
        return None
    return IcloudConfig(
        username=username,
        app_password=app_password,
        calendar_ref=os.environ.get("BAIKAL_BUSY_ICLOUD_CALENDAR", "").strip(),
    )


class BaikalBusySync:
    def __init__(self, db_path, out_path, icloud=None):
        self.db_path = db_path
        self.out_path = out_path
        self.icloud = icloud
        self.conn = None

    def connect(self):
        self.conn = sqlite3.connect(f"file:{self.db_path}?mode=ro", uri=True, timeout=30.0)
        self.conn.row_factory = sqlite3.Row

    def close(self):
        if self.conn:
            self.conn.close()

    def _operator_principal(self):
        row = self.conn.execute(
            "SELECT uri FROM principals "
            "WHERE uri LIKE 'principals/%' AND uri NOT LIKE 'principals/%/%' "
            "ORDER BY id LIMIT 1"
        ).fetchone()
        return row["uri"] if row else None

    def _calendar_data(self, principal_uri):
        rows = self.conn.execute(
            "SELECT calendardata FROM calendarobjects WHERE calendarid IN "
            "(SELECT calendarid FROM calendarinstances WHERE principaluri = ?)",
            (principal_uri,),
        ).fetchall()
        return [row["calendardata"] for row in rows]

    def _busy_blocks(self, calendar_data, window_start, window_end):
        blocks = {}
        for raw in calendar_data:
            calendar = icalendar.Calendar.from_ical(raw)
            for component in recurring_ical_events.of(calendar).between(window_start, window_end):
                if not _is_busy(component):
                    continue
                start = component["DTSTART"].dt
                end = component["DTEND"].dt
                all_day = _is_all_day(start)
                uid = _make_uid(str(component.get("UID", "")), start)
                blocks[uid] = BusyBlock(uid=uid, start=start, end=end, all_day=all_day)
        return sorted(blocks.values(), key=lambda b: (_instance_key(b.start), b.uid))

    def _render(self, blocks):
        lines = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Baikal//Busy Feed//EN"]
        for block in blocks:
            lines.append("BEGIN:VEVENT")
            lines.append(f"UID:{block.uid}")
            if block.all_day:
                lines.append(f"DTSTART;VALUE=DATE:{block.start.strftime('%Y%m%d')}")
                lines.append(f"DTEND;VALUE=DATE:{block.end.strftime('%Y%m%d')}")
            else:
                lines.append(f"DTSTART:{_to_utc(block.start).strftime('%Y%m%dT%H%M%SZ')}")
                lines.append(f"DTEND:{_to_utc(block.end).strftime('%Y%m%dT%H%M%SZ')}")
            lines.append("SUMMARY:Busy")
            lines.append("TRANSP:OPAQUE")
            lines.append("END:VEVENT")
        lines.append("END:VCALENDAR")
        return CRLF.join(lines) + CRLF

    def _write(self, content):
        out = Path(self.out_path)
        out.parent.mkdir(parents=True, exist_ok=True)
        tmp = out.with_suffix(out.suffix + ".tmp")
        tmp.write_text(content)
        os.chmod(tmp, 0o644)
        tmp.replace(out)

    def generate(self):
        self.connect()
        try:
            principal = self._operator_principal()
            if not principal:
                print("No operator principal found", file=sys.stderr)
                return False
            now = datetime.now(timezone.utc)
            window_start = now - timedelta(days=WINDOW_PAST_DAYS)
            window_end = now + timedelta(days=WINDOW_FUTURE_DAYS)
            calendar_data = self._calendar_data(principal)
            if self.icloud:
                calendar_data += _icloud_calendar_data(self.icloud, window_start, window_end)
            blocks = self._busy_blocks(calendar_data, window_start, window_end)
            self._write(self._render(blocks))
            print(f"Wrote {len(blocks)} busy blocks to {self.out_path}")
            return True
        finally:
            self.close()


def main():
    db_path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DB_PATH
    out_path = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_OUT_PATH
    sync = BaikalBusySync(db_path, out_path, icloud=_icloud_config_from_env())
    sys.exit(0 if sync.generate() else 1)


if __name__ == "__main__":
    main()
