import importlib.util
import re
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

SCRIPT = Path(__file__).parent / "baikal-busy-sync.py"
spec = importlib.util.spec_from_file_location("baikal_busy_sync", SCRIPT)
busy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(busy)

PRINCIPAL = "principals/operator"
DETAIL_TOKENS = ("LOCATION", "DESCRIPTION", "ATTENDEE", "ORGANIZER", "URL", "CATEGORIES", "CLASS")


def build_db(path, calendars):
    conn = sqlite3.connect(path)
    conn.executescript(
        """
        CREATE TABLE principals (id INTEGER PRIMARY KEY, uri TEXT, email TEXT, displayname TEXT);
        CREATE TABLE calendars (id INTEGER PRIMARY KEY, synctoken INT, components TEXT);
        CREATE TABLE calendarinstances (
            id INTEGER PRIMARY KEY, calendarid INT, principaluri TEXT, uri TEXT,
            displayname TEXT, transparent INT
        );
        CREATE TABLE calendarobjects (
            id INTEGER PRIMARY KEY, calendarid INT, uri TEXT, calendardata TEXT, componenttype TEXT
        );
        """
    )
    conn.execute(
        "INSERT INTO principals (uri, email, displayname) VALUES (?, ?, ?)",
        (PRINCIPAL, "operator@example.com", "Operator"),
    )
    for cal_uri, objects in calendars:
        cur = conn.execute("INSERT INTO calendars (synctoken, components) VALUES (1, 'VEVENT')")
        calendar_id = cur.lastrowid
        conn.execute(
            "INSERT INTO calendarinstances (calendarid, principaluri, uri, displayname, transparent) "
            "VALUES (?, ?, ?, ?, 0)",
            (calendar_id, PRINCIPAL, cal_uri, cal_uri),
        )
        for obj_uri, ics in objects:
            conn.execute(
                "INSERT INTO calendarobjects (calendarid, uri, calendardata, componenttype) VALUES (?, ?, ?, 'VEVENT')",
                (calendar_id, obj_uri, ics),
            )
    conn.commit()
    conn.close()


def make_event(uid, dtstart, dtend, summary="Secret meeting", extra_lines=()):
    lines = [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//test//EN",
        "BEGIN:VEVENT",
        f"UID:{uid}",
        "DTSTAMP:20260101T000000Z",
        f"DTSTART:{dtstart}",
        f"DTEND:{dtend}",
        f"SUMMARY:{summary}",
        *extra_lines,
        "END:VEVENT",
        "END:VCALENDAR",
    ]
    return "\r\n".join(lines) + "\r\n"


def make_all_day(uid, start_date, end_date, summary="Vacation"):
    lines = [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//test//EN",
        "BEGIN:VEVENT",
        f"UID:{uid}",
        "DTSTAMP:20260101T000000Z",
        f"DTSTART;VALUE=DATE:{start_date}",
        f"DTEND;VALUE=DATE:{end_date}",
        f"SUMMARY:{summary}",
        "END:VEVENT",
        "END:VCALENDAR",
    ]
    return "\r\n".join(lines) + "\r\n"


def utc_stamp(dt):
    return dt.astimezone(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def parse_feed(text):
    text = text.replace("\r\n", "\n")
    blocks = re.findall(r"BEGIN:VEVENT\n(.*?)END:VEVENT", text, re.DOTALL)
    events = []
    for block in blocks:
        fields = {}
        for line in block.split("\n"):
            if not line:
                continue
            key, _, value = line.partition(":")
            fields[key] = value
        events.append(fields)
    return events


@pytest.fixture
def db_path(tmp_path):
    return str(tmp_path / "db.sqlite")


@pytest.fixture
def out_path(tmp_path):
    return str(tmp_path / "busy" / "busy.ics")


def run(db_path, out_path):
    assert busy.BaikalBusySync(db_path, out_path).generate()
    return parse_feed(Path(out_path).read_text())


def test_busy_event_emitted_sanitized(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    build_db(db_path, [("work", [("a.ics", make_event("evt-1", utc_stamp(start), utc_stamp(end)))])])

    events = run(db_path, out_path)

    assert len(events) == 1
    ev = events[0]
    assert ev["SUMMARY"] == "Busy"
    assert ev["TRANSP"] == "OPAQUE"
    assert ev["DTSTART"] == utc_stamp(start)
    assert ev["DTEND"] == utc_stamp(end)
    assert re.fullmatch(r"[0-9a-f]{32}", ev["UID"])


def test_transparent_event_excluded(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    ics = make_event("free-1", utc_stamp(start), utc_stamp(end), extra_lines=("TRANSP:TRANSPARENT",))
    build_db(db_path, [("work", [("a.ics", ics)])])

    assert run(db_path, out_path) == []


def test_cancelled_event_excluded(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    ics = make_event("cx-1", utc_stamp(start), utc_stamp(end), extra_lines=("STATUS:CANCELLED",))
    build_db(db_path, [("work", [("a.ics", ics)])])

    assert run(db_path, out_path) == []


def test_birthdays_calendar_auto_excluded(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    bday = make_all_day("b-1", start.strftime("%Y%m%d"), (start + timedelta(days=1)).strftime("%Y%m%d"))
    bday = bday.replace("END:VEVENT", "RRULE:FREQ=YEARLY\r\nTRANSP:TRANSPARENT\r\nEND:VEVENT")
    build_db(db_path, [("birthdays", [("b.ics", bday)])])

    assert run(db_path, out_path) == []


def test_all_day_multi_day_block(db_path, out_path):
    start = (datetime.now(timezone.utc) + timedelta(days=3)).date()
    end = start + timedelta(days=3)
    ics = make_all_day("ad-1", start.strftime("%Y%m%d"), end.strftime("%Y%m%d"))
    build_db(db_path, [("work", [("a.ics", ics)])])

    events = run(db_path, out_path)

    assert len(events) == 1
    assert events[0]["DTSTART;VALUE=DATE"] == start.strftime("%Y%m%d")
    assert events[0]["DTEND;VALUE=DATE"] == end.strftime("%Y%m%d")
    assert events[0]["SUMMARY"] == "Busy"


def test_simple_recurrence_expanded(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    ics = make_event("rec-1", utc_stamp(start), utc_stamp(end), extra_lines=("RRULE:FREQ=DAILY;COUNT=3",))
    build_db(db_path, [("work", [("a.ics", ics)])])

    events = run(db_path, out_path)

    assert len(events) == 3
    starts = sorted(e["DTSTART"] for e in events)
    expected = sorted(utc_stamp(start + timedelta(days=n)) for n in range(3))
    assert starts == expected


def test_exdate_removes_instance(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    skip = utc_stamp(start + timedelta(days=1))
    ics = make_event(
        "ex-1",
        utc_stamp(start),
        utc_stamp(end),
        extra_lines=("RRULE:FREQ=DAILY;COUNT=3", f"EXDATE:{skip}"),
    )
    build_db(db_path, [("work", [("a.ics", ics)])])

    events = run(db_path, out_path)

    assert len(events) == 2
    assert skip not in {e["DTSTART"] for e in events}


def test_recurrence_id_override_cancels_one(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    moved = start + timedelta(days=1)
    override = (
        "BEGIN:VEVENT\r\n"
        "UID:ovr-1\r\n"
        "DTSTAMP:20260101T000000Z\r\n"
        f"RECURRENCE-ID:{utc_stamp(moved)}\r\n"
        f"DTSTART:{utc_stamp(moved)}\r\n"
        f"DTEND:{utc_stamp(moved + timedelta(hours=1))}\r\n"
        "SUMMARY:Cancelled occurrence\r\n"
        "STATUS:CANCELLED\r\n"
        "END:VEVENT\r\n"
    )
    ics = make_event("ovr-1", utc_stamp(start), utc_stamp(end), extra_lines=("RRULE:FREQ=DAILY;COUNT=3",))
    ics = ics.replace("END:VCALENDAR\r\n", override + "END:VCALENDAR\r\n")
    build_db(db_path, [("work", [("a.ics", ics)])])

    events = run(db_path, out_path)

    assert len(events) == 2
    assert utc_stamp(moved) not in {e["DTSTART"] for e in events}


def test_window_bounds(db_path, out_path):
    now = datetime.now(timezone.utc)
    past = now - timedelta(days=10)
    future = now + timedelta(days=120)
    inside = now + timedelta(days=5)
    calendars = [
        (
            "work",
            [
                ("p.ics", make_event("past", utc_stamp(past), utc_stamp(past + timedelta(hours=1)))),
                ("f.ics", make_event("future", utc_stamp(future), utc_stamp(future + timedelta(hours=1)))),
                ("i.ics", make_event("inside", utc_stamp(inside), utc_stamp(inside + timedelta(hours=1)))),
            ],
        )
    ]
    build_db(db_path, calendars)

    events = run(db_path, out_path)

    assert len(events) == 1
    assert events[0]["DTSTART"] == utc_stamp(inside)


def test_all_calendars_included(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    other = start + timedelta(hours=3)
    calendars = [
        ("work", [("w.ics", make_event("w-1", utc_stamp(start), utc_stamp(start + timedelta(hours=1))))]),
        ("personal", [("p.ics", make_event("p-1", utc_stamp(other), utc_stamp(other + timedelta(hours=1))))]),
    ]
    build_db(db_path, calendars)

    events = run(db_path, out_path)

    assert len(events) == 2


def test_uid_stable_across_runs(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    build_db(db_path, [("work", [("a.ics", make_event("evt-1", utc_stamp(start), utc_stamp(end)))])])

    first = Path(out_path)
    run(db_path, out_path)
    content_1 = first.read_text()
    run(db_path, out_path)
    content_2 = first.read_text()

    assert content_1 == content_2


def test_no_detail_fields_leak(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    ics = make_event(
        "leak-1",
        utc_stamp(start),
        utc_stamp(end),
        summary="Therapy with Dr. Smith",
        extra_lines=(
            "LOCATION:123 Private St",
            "DESCRIPTION:very personal",
            "ATTENDEE:mailto:someone@example.com",
            "ORGANIZER:mailto:boss@example.com",
            "URL:https://secret.example.com",
            "CATEGORIES:PERSONAL",
        ),
    )
    build_db(db_path, [("work", [("a.ics", ics)])])
    busy.BaikalBusySync(db_path, out_path).generate()
    text = Path(out_path).read_text()

    assert "Therapy" not in text
    assert "Dr. Smith" not in text
    assert "leak-1" not in text
    for token in DETAIL_TOKENS:
        assert token not in text


def test_read_only_db_not_modified(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    build_db(db_path, [("work", [("a.ics", make_event("evt-1", utc_stamp(start), utc_stamp(end)))])])
    before = Path(db_path).read_bytes()

    run(db_path, out_path)

    assert Path(db_path).read_bytes() == before
