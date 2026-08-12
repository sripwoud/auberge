import importlib.util
import re
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest import mock
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import pytest

SCRIPT = Path(__file__).parent / "baikal-busy-sync.py"
spec = importlib.util.spec_from_file_location("baikal_busy_sync", SCRIPT)
busy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(busy)

PRINCIPAL = "principals/operator"
DETAIL_TOKENS = ("LOCATION", "DESCRIPTION", "ATTENDEE", "ORGANIZER", "URL", "CATEGORIES", "CLASS")
BERLIN = ZoneInfo("Europe/Berlin")
EU_DST_RULES = (
    "BEGIN:DAYLIGHT",
    "TZOFFSETFROM:+0100",
    "TZOFFSETTO:+0200",
    "TZNAME:CEST",
    "DTSTART:19700329T020000",
    "RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU",
    "END:DAYLIGHT",
    "BEGIN:STANDARD",
    "TZOFFSETFROM:+0200",
    "TZOFFSETTO:+0100",
    "TZNAME:CET",
    "DTSTART:19701025T030000",
    "RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU",
    "END:STANDARD",
)


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


def make_zoned_event(uid, tzid, dtstart, dtend, summary="Zoned meeting", extra_lines=()):
    lines = [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//test//EN",
        "BEGIN:VTIMEZONE",
        f"TZID:{tzid}",
        *EU_DST_RULES,
        "END:VTIMEZONE",
        "BEGIN:VEVENT",
        f"UID:{uid}",
        "DTSTAMP:20260101T000000Z",
        f"DTSTART;TZID={tzid}:{dtstart}",
        f"DTEND;TZID={tzid}:{dtend}",
        f"SUMMARY:{summary}",
        *extra_lines,
        "END:VEVENT",
        "END:VCALENDAR",
    ]
    return "\r\n".join(lines) + "\r\n"


def utc_stamp(dt):
    return dt.astimezone(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def floating_stamp(dt):
    return dt.strftime("%Y%m%dT%H%M%S")


def load_busy(monkeypatch, local_tz=None):
    if local_tz is None:
        monkeypatch.delenv("BAIKAL_BUSY_LOCAL_TZ", raising=False)
    else:
        monkeypatch.setenv("BAIKAL_BUSY_LOCAL_TZ", local_tz)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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


def run(db_path, out_path, module=busy):
    assert module.BaikalBusySync(db_path, out_path).generate()
    return parse_feed(Path(out_path).read_text())


def render_between(module, db_path, out_path, calendar_data, window_start, window_end):
    sync = module.BaikalBusySync(db_path, out_path)
    return parse_feed(sync._render(sync._busy_blocks(calendar_data, window_start, window_end)))


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


def test_wal_mode_db_readable(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    end = start + timedelta(hours=1)
    build_db(db_path, [("work", [("a.ics", make_event("evt-1", utc_stamp(start), utc_stamp(end)))])])
    wal = sqlite3.connect(db_path)
    wal.execute("PRAGMA journal_mode=WAL")
    wal.close()

    events = run(db_path, out_path)

    assert len(events) == 1


def test_floating_datetime_interpreted_in_local_tz(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "Europe/Berlin")
    local = (datetime.now(BERLIN) + timedelta(days=2)).replace(minute=0, second=0, microsecond=0, tzinfo=None)
    ics = make_event("float-1", floating_stamp(local), floating_stamp(local + timedelta(hours=1)))
    build_db(db_path, [("icloud", [("a.ics", ics)])])

    events = run(db_path, out_path, module)

    assert len(events) == 1
    assert events[0]["DTSTART"] == utc_stamp(local.replace(tzinfo=BERLIN))
    assert events[0]["DTSTART"] != f"{floating_stamp(local)}Z"


def test_floating_datetime_defaults_to_utc_when_unset(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch)
    local = (datetime.now(timezone.utc) + timedelta(days=2)).replace(minute=0, second=0, microsecond=0, tzinfo=None)
    ics = make_event("float-2", floating_stamp(local), floating_stamp(local + timedelta(hours=1)))
    build_db(db_path, [("icloud", [("a.ics", ics)])])

    events = run(db_path, out_path, module)

    assert len(events) == 1
    assert events[0]["DTSTART"] == f"{floating_stamp(local)}Z"


def test_floating_datetime_crosses_dst_boundary(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "Europe/Berlin")
    ics = make_event(
        "float-dst",
        "20261021T110000",
        "20261021T120000",
        extra_lines=("RRULE:FREQ=WEEKLY;COUNT=2",),
    )
    window_start = datetime(2026, 10, 1, tzinfo=timezone.utc)
    window_end = datetime(2026, 11, 15, tzinfo=timezone.utc)

    events = render_between(module, db_path, out_path, [ics], window_start, window_end)

    assert [e["DTSTART"] for e in events] == ["20261021T090000Z", "20261028T100000Z"]


def test_floating_uid_stable_across_runs(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "Europe/Berlin")
    local = (datetime.now(BERLIN) + timedelta(days=2)).replace(minute=0, second=0, microsecond=0, tzinfo=None)
    ics = make_event("float-uid", floating_stamp(local), floating_stamp(local + timedelta(hours=1)))
    build_db(db_path, [("icloud", [("a.ics", ics)])])

    run(db_path, out_path, module)
    content_1 = Path(out_path).read_text()
    run(db_path, out_path, module)

    assert Path(out_path).read_text() == content_1


def test_invalid_local_tz_raises(monkeypatch):
    with pytest.raises(ZoneInfoNotFoundError):
        load_busy(monkeypatch, "Europe/Nowhere")


def test_tzid_event_converted_to_utc(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "Europe/Berlin")
    ics = make_zoned_event("tzid-1", "Europe/Berlin", "20260813T110000", "20260813T120000")
    window_start = datetime(2026, 8, 1, tzinfo=timezone.utc)
    window_end = datetime(2026, 9, 1, tzinfo=timezone.utc)

    events = render_between(module, db_path, out_path, [ics], window_start, window_end)

    assert [e["DTSTART"] for e in events] == ["20260813T090000Z"]


def test_tzid_event_ignores_local_tz(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "America/New_York")
    ics = make_zoned_event("tzid-2", "Europe/Berlin", "20260813T110000", "20260813T120000")
    window_start = datetime(2026, 8, 1, tzinfo=timezone.utc)
    window_end = datetime(2026, 9, 1, tzinfo=timezone.utc)

    events = render_between(module, db_path, out_path, [ics], window_start, window_end)

    assert [e["DTSTART"] for e in events] == ["20260813T090000Z"]


def test_tzid_event_crosses_dst_boundary(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "Europe/Berlin")
    ics = make_zoned_event(
        "tzid-dst",
        "Europe/Madrid",
        "20261021T110000",
        "20261021T120000",
        extra_lines=("RRULE:FREQ=WEEKLY;COUNT=2",),
    )
    window_start = datetime(2026, 10, 1, tzinfo=timezone.utc)
    window_end = datetime(2026, 11, 15, tzinfo=timezone.utc)

    events = render_between(module, db_path, out_path, [ics], window_start, window_end)

    assert [e["DTSTART"] for e in events] == ["20261021T090000Z", "20261028T100000Z"]


def test_all_day_unaffected_by_local_tz(monkeypatch, db_path, out_path):
    module = load_busy(monkeypatch, "Pacific/Kiritimati")
    start = (datetime.now(timezone.utc) + timedelta(days=3)).date()
    end = start + timedelta(days=2)
    ics = make_all_day("ad-tz", start.strftime("%Y%m%d"), end.strftime("%Y%m%d"))
    build_db(db_path, [("work", [("a.ics", ics)])])

    events = run(db_path, out_path, module)

    assert len(events) == 1
    assert events[0]["DTSTART;VALUE=DATE"] == start.strftime("%Y%m%d")
    assert events[0]["DTEND;VALUE=DATE"] == end.strftime("%Y%m%d")


class FakeCalObject:
    def __init__(self, data):
        self.data = data


class FakeCalendar:
    def __init__(self, name, objects):
        self._name = name
        self._objects = objects

    def get_display_name(self):
        return self._name

    def search(self, start, end, event, expand):
        return [FakeCalObject(ics) for ics in self._objects]


def fake_caldav_client(calendars, by_url=None):
    client = mock.MagicMock()
    client.principal.return_value.calendars.return_value = calendars
    client.calendar.return_value = by_url
    return client


def window():
    now = datetime.now(timezone.utc)
    return now - timedelta(days=1), now + timedelta(days=60)


def test_icloud_ics_merged_and_sanitized(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    icloud_start = start + timedelta(hours=4)
    baikal_ics = make_event("baikal-1", utc_stamp(start), utc_stamp(start + timedelta(hours=1)))
    icloud_ics = make_event(
        "icloud-1",
        utc_stamp(icloud_start),
        utc_stamp(icloud_start + timedelta(hours=1)),
        summary="Dentist appointment",
        extra_lines=("LOCATION:Clinic", "ATTENDEE:mailto:doc@example.com"),
    )
    sync = busy.BaikalBusySync(db_path, out_path)
    ws, we = window()

    blocks = sync._busy_blocks([baikal_ics, icloud_ics], ws, we)
    text = sync._render(blocks)

    assert text.count("SUMMARY:Busy") == 2
    assert "Dentist" not in text
    assert "Clinic" not in text
    assert "icloud-1" not in text


def test_icloud_calendar_name_filter():
    start = datetime.now(timezone.utc) + timedelta(days=3)
    shared = make_event("s-1", utc_stamp(start), utc_stamp(start + timedelta(hours=1)))
    other = make_event("o-1", utc_stamp(start), utc_stamp(start + timedelta(hours=1)))
    calendars = [FakeCalendar("Personal", [other]), FakeCalendar("Shared Team", [shared])]
    config = busy.IcloudConfig(username="u@me.com", app_password="pw", calendar_ref="shared team")
    ws, we = window()

    with mock.patch("caldav.DAVClient", return_value=fake_caldav_client(calendars)):
        data = busy._icloud_calendar_data(config, ws, we)

    assert data == [shared]


def test_icloud_missing_named_calendar_raises():
    calendars = [FakeCalendar("Personal", [])]
    config = busy.IcloudConfig(username="u@me.com", app_password="pw", calendar_ref="Nonexistent")
    ws, we = window()

    with mock.patch("caldav.DAVClient", return_value=fake_caldav_client(calendars)):
        with pytest.raises(RuntimeError, match="iCloud calendar not found.*Personal"):
            busy._icloud_calendar_data(config, ws, we)


def test_icloud_url_ref_addresses_directly():
    start = datetime.now(timezone.utc) + timedelta(days=3)
    shared = make_event("u-1", utc_stamp(start), utc_stamp(start + timedelta(hours=1)))
    url = "https://p108-caldav.icloud.com/123/calendars/abc/"
    config = busy.IcloudConfig(username="u@me.com", app_password="pw", calendar_ref=url)
    client = fake_caldav_client([], by_url=FakeCalendar("Shared", [shared]))
    ws, we = window()

    with mock.patch("caldav.DAVClient", return_value=client) as dav_client:
        data = busy._icloud_calendar_data(config, ws, we)

    assert data == [shared]
    dav_client.assert_called_once_with(url=url, username="u@me.com", password="pw")
    client.calendar.assert_called_once_with(url=url)
    client.principal.assert_not_called()


def test_icloud_config_from_env_absent(monkeypatch):
    monkeypatch.delenv("BAIKAL_BUSY_ICLOUD_USERNAME", raising=False)
    monkeypatch.delenv("BAIKAL_BUSY_ICLOUD_APP_PASSWORD", raising=False)

    assert busy._icloud_config_from_env() is None


def test_icloud_config_from_env_present(monkeypatch):
    monkeypatch.setenv("BAIKAL_BUSY_ICLOUD_USERNAME", "u@me.com")
    monkeypatch.setenv("BAIKAL_BUSY_ICLOUD_APP_PASSWORD", "app-pw")
    monkeypatch.setenv("BAIKAL_BUSY_ICLOUD_CALENDAR", "Shared Team")

    config = busy._icloud_config_from_env()

    assert config == busy.IcloudConfig(username="u@me.com", app_password="app-pw", calendar_ref="Shared Team")


def test_generate_merges_icloud_source(db_path, out_path):
    start = datetime.now(timezone.utc) + timedelta(days=2)
    icloud_start = start + timedelta(hours=6)
    build_db(
        db_path, [("work", [("a.ics", make_event("b-1", utc_stamp(start), utc_stamp(start + timedelta(hours=1))))])]
    )
    icloud_ics = make_event("ic-1", utc_stamp(icloud_start), utc_stamp(icloud_start + timedelta(hours=1)))
    config = busy.IcloudConfig(username="u@me.com", app_password="pw", calendar_ref="Shared")

    with mock.patch.object(busy, "_icloud_calendar_data", return_value=[icloud_ics]):
        assert busy.BaikalBusySync(db_path, out_path, icloud=config).generate()

    events = parse_feed(Path(out_path).read_text())
    assert len(events) == 2
