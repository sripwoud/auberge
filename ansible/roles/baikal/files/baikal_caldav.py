#!/usr/bin/env python3

"""Shared CalDAV writing machinery for the Baikal sync scripts.

Both sync scripts write generated events straight into Baikal's SQLite store,
which means both have to honour sabre/dav's synctoken contract: every insert,
update and delete needs a matching `calendarchanges` row and a bumped
`calendars.synctoken`, or clients silently stop syncing without erroring. That
contract is the reason this module exists rather than being copied twice.
"""

import hashlib
import re
import sqlite3
from datetime import datetime, timezone
from typing import NamedTuple

CRLF = "\r\n"
MAX_INT32 = 2147483647
FOLD_LIMIT = 75
DYNAMIC_PREFIXES = ("DTSTAMP:", "CREATED:", "LAST-MODIFIED:")

OPERATION_INSERT = 1
OPERATION_UPDATE = 2
OPERATION_DELETE = 3

_ESCAPED = re.compile(r"\\(.)")
_UNESCAPES = {"n": "\n", "N": "\n"}


class CalendarObject(NamedTuple):
    """One event row as `write_events` needs it.

    `first_occurrence` and `last_occurrence` are unix timestamps sabre/dav
    answers time-range queries from; a recurring event passes MAX_INT32 as the
    last, a bounded one passes its real end.
    """

    uri: str
    ical: str
    first_occurrence: int
    last_occurrence: int


def operator_principal(db):
    """The operator's own principal, from a connection or a cursor.

    `NOT LIKE 'principals/%/%'` excludes sabre/dav's `calendar-proxy-read` and
    `-write` sub-principals, which a loose `LIKE 'principals/%'` can return
    first and which own no calendars.
    """
    row = db.execute(
        "SELECT uri FROM principals "
        "WHERE uri LIKE 'principals/%' AND uri NOT LIKE 'principals/%/%' "
        "ORDER BY id LIMIT 1"
    ).fetchone()
    return row["uri"] if row else None


def stable_ical(ical_data):
    """The comparable part of an event: everything but the per-run stamps."""
    lines = ical_data.replace(CRLF, "\n").split("\n")
    return "\n".join(line for line in lines if not line.startswith(DYNAMIC_PREFIXES))


def as_text(value):
    """Decode a column sqlite typed as BLOB rather than TEXT.

    Baikal's own writes land as TEXT, but a restore, a manual fix or a client
    round-trip can leave a BLOB behind, and comparing bytes to str skips the
    row silently (#616).
    """
    if not isinstance(value, bytes):
        return value
    try:
        return value.decode("utf-8")
    except UnicodeDecodeError:
        return value.decode("latin-1")


def extract_name(vcard_data):
    """The contact's display name, falling back to the structured `N` field."""
    if not vcard_data:
        return "Unknown"
    match = re.search(r"FN:([^\r\n]+)", vcard_data)
    if match:
        return match.group(1).strip()
    match = re.search(r"N:([^;]+);([^;\r\n]*)", vcard_data)
    if match:
        return f"{match.group(2).strip()} {match.group(1).strip()}".strip()
    return "Unknown"


def unescape_text(value):
    """Read an RFC 6350 / RFC 2426 TEXT value back to plain text."""
    return _ESCAPED.sub(lambda match: _UNESCAPES.get(match.group(1), match.group(1)), value)


def escape_text(value):
    """Escape plain text for an RFC 5545 TEXT value."""
    escaped = value.replace("\\", "\\\\").replace(";", "\\;").replace(",", "\\,")
    return escaped.replace(CRLF, "\\n").replace("\n", "\\n").replace("\r", "\\n")


def fold(line):
    """Fold one content line to 75 octets, never splitting a UTF-8 sequence."""
    data = line.encode("utf-8")
    if len(data) <= FOLD_LIMIT:
        return line

    chunks = []
    start = 0
    budget = FOLD_LIMIT
    while start < len(data):
        end = min(start + budget, len(data))
        while end < len(data) and data[end] & 0xC0 == 0x80:
            end -= 1
        chunks.append(data[start:end].decode("utf-8"))
        start = end
        budget = FOLD_LIMIT - 1
    return (CRLF + " ").join(chunks)


def render(lines):
    return CRLF.join(fold(line) for line in lines) + CRLF


class CalendarSync:
    """A generated calendar under the operator's principal.

    Subclasses declare which calendar they own and build the events; this class
    owns the connection, the calendar row, and the insert / update-if-changed /
    sweep-stale loop.
    """

    uid_prefix = ""
    calendar_uri = ""
    calendar_name = ""
    calendar_description = ""

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

    def _make_uid(self, contact_uri):
        return hashlib.sha256(f"{self.uid_prefix}{contact_uri}".encode()).hexdigest()[:32]

    def _get_or_create_calendar(self, principal_uri):
        cursor = self.conn.cursor()
        cursor.execute(
            "SELECT calendarid FROM calendarinstances WHERE principaluri = ? AND uri = ?",
            (principal_uri, self.calendar_uri),
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
            VALUES (?, ?, 1, ?, ?, ?, 0)""",
            (
                self.calendar_id,
                principal_uri,
                self.calendar_name,
                self.calendar_uri,
                self.calendar_description,
            ),
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

    def _existing_events(self, cursor):
        cursor.execute(
            "SELECT uri, calendardata FROM calendarobjects WHERE calendarid = ?",
            (self.calendar_id,),
        )
        return {row["uri"]: as_text(row["calendardata"]) for row in cursor.fetchall()}

    def write_events(self, cursor, objects):
        """Insert new events, rewrite drifted ones, delete the rest.

        The sweep is what makes the calendar a projection of the address book:
        an event whose source card lost its trigger disappears on the next run.
        """
        existing = self._existing_events(cursor)
        now_ts = int(datetime.now(timezone.utc).timestamp())
        written = 0
        seen = set()

        for obj in objects:
            seen.add(obj.uri)
            written += 1
            uid = obj.uri.removesuffix(".ics")
            etag = hashlib.sha256(obj.ical.encode("utf-8")).hexdigest()
            size = len(obj.ical.encode("utf-8"))

            if obj.uri in existing:
                if stable_ical(existing[obj.uri]) == stable_ical(obj.ical):
                    continue
                cursor.execute(
                    """UPDATE calendarobjects
                    SET calendardata = ?, lastmodified = ?, etag = ?, size = ?, uid = ?
                    WHERE calendarid = ? AND uri = ?""",
                    (obj.ical, now_ts, etag, size, uid, self.calendar_id, obj.uri),
                )
                self._bump_synctoken(cursor, obj.uri, OPERATION_UPDATE)
            else:
                cursor.execute(
                    """INSERT INTO calendarobjects
                    (calendarid, uri, calendardata, lastmodified, etag, size, componenttype, firstoccurence, lastoccurence, uid)
                    VALUES (?, ?, ?, ?, ?, ?, 'VEVENT', ?, ?, ?)""",
                    (
                        self.calendar_id,
                        obj.uri,
                        obj.ical,
                        now_ts,
                        etag,
                        size,
                        obj.first_occurrence,
                        obj.last_occurrence,
                        uid,
                    ),
                )
                self._bump_synctoken(cursor, obj.uri, OPERATION_INSERT)

        for stale_uri in set(existing) - seen:
            cursor.execute(
                "DELETE FROM calendarobjects WHERE calendarid = ? AND uri = ?",
                (self.calendar_id, stale_uri),
            )
            self._bump_synctoken(cursor, stale_uri, OPERATION_DELETE)

        return written
