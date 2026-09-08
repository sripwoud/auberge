"""The shared CalDAV machinery, fenced where the sync suites do not reach.

Extracting `baikal_sync` made one copy of sabre/dav's synctoken contract out
of two, which is why it needs its own suite: a mutation that empties
`_bump_synctoken` leaves both sync suites green, because neither reads
`calendarchanges` or `calendars.synctoken`. That divergence is the silent one —
a client stops syncing and nothing errors — so it is asserted here directly.
"""

import sqlite3

import pytest

import baikal_sync

PRINCIPAL = "principals/operator"
PROXY = "principals/operator/calendar-proxy-write"

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
"""


class Sync(baikal_sync.CalendarSync):
    uid_prefix = "test-"
    calendar_uri = "probe"
    calendar_name = "Probe"
    calendar_description = "A calendar the fence owns"


def event(uri, summary):
    return baikal_sync.CalendarObject(
        uri=uri,
        ical=baikal_sync.render(["BEGIN:VCALENDAR", "BEGIN:VEVENT", f"SUMMARY:{summary}", "END:VEVENT", "END:VCALENDAR"]),
        first_occurrence=0,
        last_occurrence=baikal_sync.MAX_INT32,
    )


@pytest.fixture
def sync(tmp_path):
    db_path = str(tmp_path / "db.sqlite")
    seed = sqlite3.connect(db_path)
    seed.executescript(SCHEMA)
    # The proxy is inserted FIRST, so it holds the lower id. `ORDER BY id LIMIT 1`
    # over a loose `LIKE 'principals/%'` would return it, which makes every
    # assertion below a live check on the sub-principal exclusion rather than an
    # accident of insertion order.
    for uri in (PROXY, PRINCIPAL):
        seed.execute("INSERT INTO principals (uri) VALUES (?)", (uri,))
    seed.commit()
    seed.close()

    sync = Sync(db_path)
    sync.connect()
    sync._get_or_create_calendar(PRINCIPAL)
    yield sync
    sync.close()


def write(sync, objects):
    cursor = sync.conn.cursor()
    written = sync.write_events(cursor, objects)
    sync.conn.commit()
    return written


def changes(sync):
    rows = sync.conn.execute("SELECT uri, synctoken, operation FROM calendarchanges ORDER BY id").fetchall()
    return [(row["uri"], row["synctoken"], row["operation"]) for row in rows]


def synctoken(sync):
    return sync.conn.execute("SELECT synctoken FROM calendars WHERE id = ?", (sync.calendar_id,)).fetchone()[0]


def test_an_insert_logs_a_change_and_bumps_the_synctoken(sync):
    before = synctoken(sync)

    write(sync, [event("a.ics", "One")])

    assert changes(sync) == [("a.ics", before + 1, baikal_sync.OPERATION_INSERT)]
    assert synctoken(sync) == before + 1


def test_a_rewrite_logs_an_update_and_bumps_the_synctoken(sync):
    write(sync, [event("a.ics", "One")])
    after_insert = synctoken(sync)

    write(sync, [event("a.ics", "Two")])

    assert changes(sync)[-1] == ("a.ics", after_insert + 1, baikal_sync.OPERATION_UPDATE)
    assert synctoken(sync) == after_insert + 1


def test_the_sweep_logs_a_delete_and_bumps_the_synctoken(sync):
    write(sync, [event("a.ics", "One")])
    after_insert = synctoken(sync)

    write(sync, [])

    assert changes(sync)[-1] == ("a.ics", after_insert + 1, baikal_sync.OPERATION_DELETE)
    assert synctoken(sync) == after_insert + 1


def test_an_unchanged_event_logs_nothing(sync):
    write(sync, [event("a.ics", "One")])
    after_insert = (changes(sync), synctoken(sync))

    write(sync, [event("a.ics", "One")])

    assert (changes(sync), synctoken(sync)) == after_insert


def test_the_principal_query_skips_sub_principals(sync):
    assert baikal_sync.operator_principal(sync.conn) == PRINCIPAL


def test_a_second_run_reuses_the_calendar_row(sync):
    first = sync.calendar_id

    sync.calendar_id = None
    sync._get_or_create_calendar(PRINCIPAL)

    assert sync.calendar_id == first
    assert sync.conn.execute("SELECT COUNT(*) FROM calendars").fetchone()[0] == 1


@pytest.mark.parametrize(
    ("plain", "escaped"),
    [
        ("Anna", "Anna"),
        ("Doe, Jane", "Doe\\, Jane"),
        ("a;b", "a\\;b"),
        ("back\\slash", "back\\\\slash"),
        ("one\ntwo", "one\\ntwo"),
        ("one\r\ntwo", "one\\ntwo"),
        ("all\\ of, it; here", "all\\\\ of\\, it\\; here"),
    ],
)
def test_escape_text_covers_every_rfc5545_special(plain, escaped):
    assert baikal_sync.escape_text(plain) == escaped


@pytest.mark.parametrize(
    ("stored", "plain"),
    [
        (r"Call her\, ask", "Call her, ask"),
        (r"one\ntwo", "one\ntwo"),
        (r"one\Ntwo", "one\ntwo"),
        (r"a\;b", "a;b"),
        (r"back\\slash", "back\\slash"),
        (r"literal\\n", "literal\\n"),
    ],
)
def test_unescape_text_reads_a_vcard_text_value(stored, plain):
    assert baikal_sync.unescape_text(stored) == plain


def test_a_vcard_note_round_trips_through_ical_escaping():
    stored = r"Ask about Lisboa\, then Porto\nBring the book"

    assert baikal_sync.escape_text(baikal_sync.unescape_text(stored)) == stored


@pytest.mark.parametrize(
    ("folded", "plain"),
    [
        ("NOTE:one\r\n two", "NOTE:onetwo"),
        ("NOTE:one\r\n\ttwo", "NOTE:onetwo"),
        ("NOTE:one\n two", "NOTE:onetwo"),
        ("NOTE:one\r two", "NOTE:onetwo"),
        ("NOTE:a\r\n b\r\n c", "NOTE:abc"),
        ("NOTE:keeps  the second space\r\n  here", "NOTE:keeps  the second space here"),
        ("NOTE:one\r\nFN:two", "NOTE:one\r\nFN:two"),
        ("NOTE:no folds here", "NOTE:no folds here"),
    ],
)
def test_unfold_rejoins_a_folded_content_line(folded, plain):
    assert baikal_sync.unfold(folded) == plain


def test_unfold_is_the_inverse_of_fold():
    line = "DESCRIPTION:" + "the very long story of the co-op " * 6

    assert baikal_sync.unfold(baikal_sync.fold(line)) == line


def test_unfold_is_the_inverse_of_fold_for_multibyte_text():
    line = "DESCRIPTION:" + "café und kuchen mit Renée " * 6

    assert baikal_sync.unfold(baikal_sync.fold(line)) == line


def test_folding_keeps_every_line_within_75_octets():
    folded = baikal_sync.fold("DESCRIPTION:" + "x" * 200)

    assert all(len(line.encode("utf-8")) <= baikal_sync.FOLD_LIMIT for line in folded.split(baikal_sync.CRLF))
    assert folded.replace(baikal_sync.CRLF + " ", "") == "DESCRIPTION:" + "x" * 200


def test_folding_never_splits_a_utf8_sequence():
    folded = baikal_sync.fold("DESCRIPTION:" + "é" * 200)

    for line in folded.split(baikal_sync.CRLF):
        assert len(line.encode("utf-8")) <= baikal_sync.FOLD_LIMIT
    assert folded.replace(baikal_sync.CRLF + " ", "") == "DESCRIPTION:" + "é" * 200


def test_a_short_line_is_not_folded():
    assert baikal_sync.fold("SUMMARY:Reach out: Anna") == "SUMMARY:Reach out: Anna"


def test_stable_ical_ignores_the_per_run_stamps():
    def ics(stamp):
        return baikal_sync.render(["BEGIN:VEVENT", f"DTSTAMP:{stamp}", "SUMMARY:One", "END:VEVENT"])

    assert baikal_sync.stable_ical(ics("20260101T000000Z")) == baikal_sync.stable_ical(ics("20270101T000000Z"))


def test_as_text_decodes_a_blob_column():
    assert baikal_sync.as_text("Renée".encode("utf-8")) == "Renée"
    assert baikal_sync.as_text("Renée".encode("latin-1")) == "Renée"
    assert baikal_sync.as_text("Renée") == "Renée"
