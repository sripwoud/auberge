import importlib.util
import sqlite3
from pathlib import Path

import pytest

SCRIPT = Path(__file__).parent / "baikal-birthday-sync.py"
spec = importlib.util.spec_from_file_location("baikal_birthday_sync", SCRIPT)
birthday = importlib.util.module_from_spec(spec)
spec.loader.exec_module(birthday)

PRINCIPAL = "principals/operator"

SCHEMA = """
CREATE TABLE principals (id INTEGER PRIMARY KEY, uri TEXT);
CREATE TABLE calendars (id INTEGER PRIMARY KEY, synctoken INT, components TEXT);
CREATE TABLE calendarinstances (
    id INTEGER PRIMARY KEY, calendarid INT, principaluri TEXT, access INT,
    displayname TEXT, uri TEXT, description TEXT, transparent INT
);
CREATE TABLE calendarchanges (
    id INTEGER PRIMARY KEY, uri TEXT, synctoken INT, calendarid INT, operation INT
);
CREATE TABLE calendarobjects (
    id INTEGER PRIMARY KEY, calendarid INT, uri TEXT, calendardata BLOB, lastmodified INT,
    etag TEXT, size INT, componenttype TEXT, firstoccurence INT, lastoccurence INT, uid TEXT
);
CREATE TABLE cards (id INTEGER PRIMARY KEY, uri TEXT, carddata BLOB);
"""


@pytest.fixture
def db_path(tmp_path):
    return str(tmp_path / "db.sqlite")


def make_vcard(name, bday, params=""):
    return "\r\n".join(["BEGIN:VCARD", "VERSION:3.0", f"FN:{name}", f"BDAY{params}:{bday}", "END:VCARD"]) + "\r\n"


def make_vcard_without_bday(name):
    return "\r\n".join(["BEGIN:VCARD", "VERSION:3.0", f"FN:{name}", "END:VCARD"]) + "\r\n"


def event_uri(card_uri):
    return birthday.BaikalBirthdaySync("")._make_uid(card_uri) + ".ics"


def parse_bday(value, params=""):
    return birthday.BaikalBirthdaySync("")._parse_bday(make_vcard("X", value, params))


def build_db(db_path, cards):
    conn = sqlite3.connect(db_path)
    conn.executescript(SCHEMA)
    conn.execute("INSERT INTO principals (uri) VALUES (?)", (PRINCIPAL,))
    for uri, carddata in cards:
        conn.execute("INSERT INTO cards (uri, carddata) VALUES (?, ?)", (uri, carddata))
    conn.commit()
    conn.close()


def run(db_path):
    sync = birthday.BaikalBirthdaySync(db_path)
    try:
        assert sync.sync_birthdays()
    finally:
        sync.close()


def calendar_objects(db_path):
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    rows = conn.execute("SELECT uri, calendardata, typeof(calendardata) AS kind FROM calendarobjects").fetchall()
    conn.close()
    return {row["uri"]: row for row in rows}


def store_as_blob(db_path, table, column, uri, text):
    conn = sqlite3.connect(db_path)
    conn.execute(f"UPDATE {table} SET {column} = ? WHERE uri = ?", (text.encode("utf-8"), uri))
    conn.commit()
    kind = conn.execute(f"SELECT typeof({column}) FROM {table} WHERE uri = ?", (uri,)).fetchone()[0]
    conn.close()
    assert kind == "blob"


def store_event(db_path, uri, calendardata):
    conn = sqlite3.connect(db_path)
    conn.execute("UPDATE calendarobjects SET calendardata = ? WHERE uri = ?", (calendardata, uri))
    conn.commit()
    conn.close()


def insert_orphan_event(db_path, uri):
    conn = sqlite3.connect(db_path)
    calendar_id = conn.execute("SELECT calendarid FROM calendarinstances WHERE uri = 'birthdays'").fetchone()[0]
    conn.execute(
        "INSERT INTO calendarobjects (calendarid, uri, calendardata, lastmodified, etag, size, componenttype, "
        "firstoccurence, lastoccurence, uid) VALUES (?, ?, ?, 0, '', 0, 'VEVENT', 0, 0, '')",
        (calendar_id, uri, "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n"),
    )
    conn.commit()
    conn.close()


def test_text_contacts_sync(db_path):
    build_db(db_path, [("dave.vcf", make_vcard("Dave", "1991-01-05")), ("erin.vcf", make_vcard("Erin", "1993-09-21"))])

    run(db_path)

    objects = calendar_objects(db_path)
    assert set(objects) == {event_uri("dave.vcf"), event_uri("erin.vcf")}
    assert "SUMMARY:Dave's Birthday" in objects[event_uri("dave.vcf")]["calendardata"]


def test_sync_survives_blob_calendardata(db_path):
    build_db(db_path, [("alice.vcf", make_vcard("Alice", "1990-03-14"))])
    run(db_path)
    uri = event_uri("alice.vcf")
    store_as_blob(db_path, "calendarobjects", "calendardata", uri, calendar_objects(db_path)[uri]["calendardata"])

    run(db_path)

    assert uri in calendar_objects(db_path)


def test_blob_calendardata_is_compared_not_skipped(db_path):
    build_db(db_path, [("bob.vcf", make_vcard("Bob", "1985-07-02"))])
    run(db_path)
    uri = event_uri("bob.vcf")
    drifted = calendar_objects(db_path)[uri]["calendardata"].replace("Bob's Birthday", "Bobby's Birthday")
    store_as_blob(db_path, "calendarobjects", "calendardata", uri, drifted)

    run(db_path)

    row = calendar_objects(db_path)[uri]
    assert row["kind"] == "text"
    assert "SUMMARY:Bob's Birthday" in row["calendardata"]


def test_blob_carddata_contact_is_synced(db_path):
    build_db(db_path, [("carol.vcf", make_vcard("Carol", "1978-11-30").encode("utf-8"))])

    run(db_path)

    objects = calendar_objects(db_path)
    assert event_uri("carol.vcf") in objects
    assert "SUMMARY:Carol's Birthday" in objects[event_uri("carol.vcf")]["calendardata"]


def test_blob_carddata_event_survives_stale_cleanup(db_path):
    erin = make_vcard("Erin", "1993-09-21")
    build_db(db_path, [("dave.vcf", make_vcard("Dave", "1991-01-05")), ("erin.vcf", erin)])
    run(db_path)
    store_as_blob(db_path, "cards", "carddata", "erin.vcf", erin)

    run(db_path)

    assert set(calendar_objects(db_path)) == {event_uri("dave.vcf"), event_uri("erin.vcf")}


def test_blob_carddata_without_bday_is_ignored(db_path):
    build_db(
        db_path,
        [
            ("frank.vcf", make_vcard("Frank", "1980-02-29")),
            ("gina.vcf", make_vcard_without_bday("Gina").encode("utf-8")),
        ],
    )

    run(db_path)

    assert set(calendar_objects(db_path)) == {event_uri("frank.vcf")}


def test_latin1_carddata_does_not_abort_the_run(db_path):
    latin1 = make_vcard("Renée", "1988-04-17").encode("latin-1")
    build_db(db_path, [("hank.vcf", make_vcard("Hank", "1975-06-11")), ("renee.vcf", latin1)])

    run(db_path)

    objects = calendar_objects(db_path)
    assert set(objects) == {event_uri("hank.vcf"), event_uri("renee.vcf")}
    assert "SUMMARY:Renée's Birthday" in objects[event_uri("renee.vcf")]["calendardata"]


def test_orphan_event_is_deleted(db_path):
    build_db(db_path, [("frank.vcf", make_vcard("Frank", "1980-02-29"))])
    run(db_path)
    insert_orphan_event(db_path, "orphan.ics")

    run(db_path)

    assert set(calendar_objects(db_path)) == {event_uri("frank.vcf")}


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("1991-01-05", (1, 5, 1991)),
        ("19910105", (1, 5, 1991)),
        ("--01-05", (1, 5, None)),
        ("--0105", (1, 5, None)),
        ("--12-31", (12, 31, None)),
        ("--1231", (12, 31, None)),
        ("--02-29", (2, 29, None)),
        ("--0229", (2, 29, None)),
    ],
)
def test_parse_bday_accepts_both_year_omitted_forms(value, expected):
    assert parse_bday(value) == expected


def test_parse_bday_reads_the_value_date_parameter():
    assert parse_bday("--0105", params=";VALUE=DATE") == (1, 5, None)


@pytest.mark.parametrize(
    "value",
    ["--13-01", "--1301", "--00-00", "--0000", "--02-30", "--0230", "--xxyy", "--01", "--010", "--"],
)
def test_parse_bday_rejects_malformed_year_omitted_values(value):
    assert parse_bday(value) is None


def test_year_omitted_bday_becomes_a_yearly_event(db_path):
    build_db(db_path, [("ivy.vcf", make_vcard("Ivy", "--0105", params=";VALUE=DATE"))])

    run(db_path)

    data = calendar_objects(db_path)[event_uri("ivy.vcf")]["calendardata"]
    assert f"DTSTART;VALUE=DATE:{birthday.ANCHOR_YEAR}0105" in data
    assert "RRULE:FREQ=YEARLY" in data


def test_leap_day_without_a_year_is_synced(db_path):
    build_db(db_path, [("jade.vcf", make_vcard("Jade", "--0229"))])

    run(db_path)

    data = calendar_objects(db_path)[event_uri("jade.vcf")]["calendardata"]
    assert f"DTSTART;VALUE=DATE:{birthday.ANCHOR_YEAR}0229" in data


def test_unparsable_bday_is_reported_and_does_not_abort_the_run(db_path, capsys):
    build_db(
        db_path,
        [
            ("kai.vcf", make_vcard("Kai", "1990-03-14")),
            ("lee.vcf", make_vcard("Lee", "--9999")),
            ("mia.vcf", make_vcard_without_bday("Mia")),
        ],
    )

    run(db_path)

    assert set(calendar_objects(db_path)) == {event_uri("kai.vcf")}
    assert "Unparsable BDAY values: 1" in capsys.readouterr().err


def test_event_anchored_on_the_superseded_year_is_rewritten(db_path):
    build_db(db_path, [("nils.vcf", make_vcard("Nils", "--01-05"))])
    run(db_path)
    uri = event_uri("nils.vcf")
    store_event(db_path, uri, calendar_objects(db_path)[uri]["calendardata"].replace(str(birthday.ANCHOR_YEAR), "1970"))

    run(db_path)

    assert f"DTSTART;VALUE=DATE:{birthday.ANCHOR_YEAR}0105" in calendar_objects(db_path)[uri]["calendardata"]
