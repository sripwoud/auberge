"""The reach-out nudge sync.

The fixture carries Baikal 0.12.1's real schema verbatim (`Core/Resources/Db/
SQLite/db.sql`), not the columns the script happens to read. The birthday
fixture declares `cards (id, uri, carddata)` and would have let a nudge suite
pass green against a table shape production does not have -- `lastmodified`,
the whole contact clock, is not in it, and neither is the `addressbookid NOT
NULL` that proves the sweep reads every address book.
"""

import importlib.util
import re
import sqlite3
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import icalendar
import pytest

import baikal_sync

HERE = Path(__file__).parent


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


nudge = load("baikal_nudge_sync", "baikal-nudge-sync.py")
busy = load("baikal_busy_sync", "baikal-busy-sync.py")

PRINCIPAL = "principals/operator"
PROXY = "principals/operator/calendar-proxy-write"

# Verbatim from Baikal 0.12.1 Core/Resources/Db/SQLite/db.sql.
SCHEMA = """
CREATE TABLE addressbooks (
    id integer primary key asc NOT NULL,
    principaluri text NOT NULL,
    displayname text,
    uri text NOT NULL,
    description text,
    synctoken integer DEFAULT 1 NOT NULL
);
CREATE TABLE cards (
    id integer primary key asc NOT NULL,
    addressbookid integer NOT NULL,
    carddata blob,
    uri text NOT NULL,
    lastmodified integer,
    etag text,
    size integer
);
CREATE TABLE principals (
    id INTEGER PRIMARY KEY ASC NOT NULL,
    uri TEXT NOT NULL,
    email TEXT,
    displayname TEXT,
    UNIQUE(uri)
);
CREATE TABLE calendars (
    id integer primary key asc NOT NULL,
    synctoken integer DEFAULT 1 NOT NULL,
    components text NOT NULL
);
CREATE TABLE calendarinstances (
    id integer primary key asc NOT NULL,
    calendarid integer,
    principaluri text,
    access integer,
    displayname text,
    uri text NOT NULL,
    description text,
    calendarorder integer,
    calendarcolor text,
    timezone text,
    transparent bool,
    share_href text,
    share_displayname text,
    share_invitestatus integer DEFAULT '2',
    UNIQUE (principaluri, uri),
    UNIQUE (calendarid, principaluri),
    UNIQUE (calendarid, share_href)
);
CREATE TABLE calendarchanges (
    id integer primary key asc NOT NULL,
    uri text,
    synctoken integer NOT NULL,
    calendarid integer NOT NULL,
    operation integer NOT NULL
);
CREATE TABLE calendarobjects (
    id integer primary key asc NOT NULL,
    calendardata blob NOT NULL,
    uri text NOT NULL,
    calendarid integer NOT NULL,
    lastmodified integer NOT NULL,
    etag text NOT NULL,
    size integer NOT NULL,
    componenttype text,
    firstoccurence integer,
    lastoccurence integer,
    uid text
);
"""


def vcard(name, note=None):
    lines = ["BEGIN:VCARD", "VERSION:3.0", f"FN:{name}"]
    if note is not None:
        lines.append(f"NOTE:{note}")
    lines.append("END:VCARD")
    return "\r\n".join(lines) + "\r\n"


def folded_vcard(name, note):
    """A vCard as a client actually PUTs it: content lines folded at 75 octets.

    RFC 6350 3.2 folding is not optional in practice -- iOS and DAVx5 both do
    it, so a note long enough to be worth carrying arrives split across lines.
    Reading only up to the first CRLF truncates the body mid-word and loses any
    token past the fold, which drops the nudge with no event and no count.
    """
    lines = []
    for line in ("BEGIN:VCARD", "VERSION:3.0", f"FN:{name}", f"NOTE:{note}", "END:VCARD"):
        lines.append(baikal_sync.fold(line))
    return "\r\n".join(lines) + "\r\n"


def days_ago(days):
    """A `cards.lastmodified` unix timestamp, `days` before now, in UTC."""
    return int((datetime.now(timezone.utc) - timedelta(days=days)).timestamp())


@pytest.fixture
def db_path(tmp_path):
    return str(tmp_path / "db.sqlite")


def build_db(db_path, cards, addressbooks=(1,)):
    conn = sqlite3.connect(db_path)
    conn.executescript(SCHEMA)
    # The proxy is inserted FIRST, so it holds the lower id. `ORDER BY id LIMIT 1`
    # over a loose `LIKE 'principals/%'` would return it, which makes every
    # assertion below a live check on the sub-principal exclusion rather than an
    # accident of insertion order.
    for uri in (PROXY, PRINCIPAL):
        conn.execute("INSERT INTO principals (uri) VALUES (?)", (uri,))
    for book_id in addressbooks:
        conn.execute(
            "INSERT INTO addressbooks (id, principaluri, uri, displayname) VALUES (?, ?, ?, ?)",
            (book_id, PRINCIPAL, f"book{book_id}", f"Book {book_id}"),
        )
    for card in cards:
        uri, carddata, lastmodified = card[0], card[1], card[2]
        book_id = card[3] if len(card) > 3 else 1
        conn.execute(
            "INSERT INTO cards (addressbookid, uri, carddata, lastmodified, etag, size) VALUES (?, ?, ?, ?, '', ?)",
            (book_id, uri, carddata, lastmodified, len(carddata) if carddata else 0),
        )
    conn.commit()
    conn.close()


def run(db_path, default_days=nudge.DEFAULT_CADENCE_DAYS):
    sync = nudge.BaikalNudgeSync(db_path, default_days)
    try:
        assert sync.sync_nudges()
    finally:
        sync.close()


def event_uri(card_uri):
    return nudge.BaikalNudgeSync("")._make_uid(card_uri) + ".ics"


def events(db_path):
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT uri, calendardata, firstoccurence, lastoccurence FROM calendarobjects WHERE calendarid = "
        "(SELECT calendarid FROM calendarinstances WHERE uri = 'care')"
    ).fetchall()
    conn.close()
    return {row["uri"]: row for row in rows}


def ics(db_path, card_uri):
    return baikal_sync.as_text(events(db_path)[event_uri(card_uri)]["calendardata"])


def field(db_path, card_uri, name):
    """One property of the generated event, as a client sees it after unfolding."""
    parsed = icalendar.Calendar.from_ical(ics(db_path, card_uri))
    return parsed.walk("VEVENT")[0].get(name)


def dtstart(db_path, card_uri):
    return field(db_path, card_uri, "DTSTART").dt


def today():
    return datetime.now(timezone.utc).date()


def test_a_default_token_nudges_ninety_days_after_the_card_was_touched(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "Met in Palma @d"), days_ago(10))])

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf")}
    assert dtstart(db_path, "anna.vcf") == today() + timedelta(days=80)
    assert field(db_path, "anna.vcf", "SUMMARY") == "Reach out: Anna"


def test_the_calendar_is_care_named_keep_in_touch(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(1))])

    run(db_path)

    conn = sqlite3.connect(db_path)
    rows = conn.execute("SELECT uri, displayname FROM calendarinstances").fetchall()
    conn.close()
    assert rows == [("care", "Keep in touch")]


def test_a_contact_with_no_token_gets_no_nudge(db_path):
    build_db(
        db_path,
        [
            ("anna.vcf", vcard("Anna", "Met in Palma @d"), days_ago(10)),
            ("dentist.vcf", vcard("Dentist", "Cleaning every spring"), days_ago(400)),
            ("landlord.vcf", vcard("Landlord"), days_ago(900)),
        ],
    )

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf")}


@pytest.mark.parametrize("days", [30, 180, 7, 365])
def test_an_explicit_token_overrides_the_default(db_path, days):
    build_db(db_path, [("anna.vcf", vcard("Anna", f"@{days}d"), days_ago(0))])

    run(db_path)

    assert dtstart(db_path, "anna.vcf") == today() + timedelta(days=days)


def test_the_default_comes_from_the_role_parameter_not_a_constant(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(0))])

    run(db_path, default_days=45)

    assert dtstart(db_path, "anna.vcf") == today() + timedelta(days=45)


def test_a_contact_overdue_by_two_years_is_dated_today(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@90d"), days_ago(730))])

    run(db_path)

    assert dtstart(db_path, "anna.vcf") == today()


def test_an_overdue_nudge_resurfaces_on_the_next_run(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@90d"), days_ago(730))])
    run(db_path)

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf")}
    assert dtstart(db_path, "anna.vcf") == today()


def test_an_all_day_event_ends_the_following_day(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@90d"), days_ago(730))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DTEND").dt == today() + timedelta(days=1)
    assert isinstance(dtstart(db_path, "anna.vcf"), date)


def test_the_occurrence_bounds_frame_the_single_day(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@90d"), days_ago(730))])

    run(db_path)

    row = events(db_path)[event_uri("anna.vcf")]
    start = datetime.combine(today(), datetime.min.time(), timezone.utc)
    assert row["firstoccurence"] == int(start.timestamp())
    assert row["lastoccurence"] == int((start + timedelta(days=1)).timestamp())


def test_the_note_becomes_the_event_body_with_the_token_stripped(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d Met in Palma, runs the co-op"), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DESCRIPTION") == "Met in Palma, runs the co-op"


@pytest.mark.parametrize(
    ("stored_note", "rendered"),
    [
        (r"Ask about the co-op\, then Porto @d", "Ask about the co-op, then Porto"),
        (r"Two lines @d\nsecond line", "Two lines\nsecond line"),
        (r"Semi\; colon @d", "Semi; colon"),
        (r"back\\slash @d", "back\\slash"),
        (r"@d all\\ of it\, and\; more\nplus a line", "all\\ of it, and; more\nplus a line"),
        ("bare, comma @d", "bare, comma"),
        ("bare; semicolon @d", "bare; semicolon"),
    ],
)
def test_a_note_with_rfc5545_specials_still_parses_in_a_client(db_path, stored_note, rendered):
    build_db(db_path, [("anna.vcf", vcard("Anna", stored_note), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DESCRIPTION") == rendered


def test_a_long_note_is_folded_and_unfolds_to_itself(db_path):
    note = "Ask about " + "the very long story of the co-op " * 8 + "@d"
    build_db(db_path, [("anna.vcf", vcard("Anna", note), days_ago(1))])

    run(db_path)

    raw = ics(db_path, "anna.vcf")
    assert all(len(line.encode("utf-8")) <= baikal_sync.FOLD_LIMIT for line in raw.split(baikal_sync.CRLF))
    assert field(db_path, "anna.vcf", "DESCRIPTION") == note.replace(" @d", "")


def test_a_name_with_a_comma_stays_one_summary(db_path):
    build_db(db_path, [("anna.vcf", vcard("Doe, Jane", "@d"), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "SUMMARY") == "Reach out: Doe, Jane"


def test_the_event_carries_the_declared_properties(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(1))])

    run(db_path)

    raw = ics(db_path, "anna.vcf")
    assert "CATEGORIES:Nudge" in raw
    assert "CLASS:PRIVATE" in raw
    assert "VALARM" not in raw


def test_the_event_is_transparent_because_the_busy_feed_reads_every_calendar(db_path):
    """`TRANSP:TRANSPARENT` is the only thing keeping nudges out of the feed.

    `baikal-busy-sync.py` selects every calendar under the principal with no
    uri filter, so `care` is swept in, and `_is_busy` excludes an event on
    exactly this line. Names do not leak (`SUMMARY:Busy` is hardcoded), but
    without it every overdue contact publishes an all-day busy block to anyone
    holding the feed token -- and under clamping that is most days.
    """
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(1))])

    run(db_path)

    assert "TRANSP:TRANSPARENT" in ics(db_path, "anna.vcf")


def test_a_nudge_does_not_appear_in_the_generated_busy_feed(db_path, tmp_path):
    """The end-to-end form of the assertion above, over the real busy sync.

    The contact is overdue on purpose: clamping puts its event on today, inside
    the feed's `[now - 1d, now + 60d]` window, so a block's absence is the
    transparency filter and not the window missing a distant date.
    """
    build_db(db_path, [("anna.vcf", vcard("Anna", "Met in Palma @90d"), days_ago(730))])
    run(db_path)

    out = tmp_path / "busy.ics"
    feed = busy.BaikalBusySync(db_path, str(out))
    assert feed.generate()

    assert "BEGIN:VEVENT" not in out.read_text()


@pytest.mark.parametrize(
    "note",
    [
        "anna@gmail.com",
        "reach anna@gmail.com weekly",
        "anna@d.example",
        "anna@dev.io",
        "x.y@d.com",
        "@dentist appointments",
        "call @dad",
        "invoice 30d after delivery",
        "id@180days.example",
    ],
)
def test_the_token_grammar_cannot_match_an_address_or_a_word(db_path, note):
    build_db(db_path, [("anna.vcf", vcard("Anna", note), days_ago(400))])

    run(db_path)

    assert events(db_path) == {}


@pytest.mark.parametrize("note", ["@d", "@d met in Palma", "met in Palma @d", "keep in touch (@d)", "@d\\nline two"])
def test_the_token_is_read_wherever_it_sits_in_the_note(db_path, note):
    build_db(db_path, [("anna.vcf", vcard("Anna", note), days_ago(1))])

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf")}


def test_touching_a_card_moves_its_nudge_forward(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@90d"), days_ago(80))])
    run(db_path)
    before = dtstart(db_path, "anna.vcf")

    conn = sqlite3.connect(db_path)
    conn.execute("UPDATE cards SET lastmodified = ? WHERE uri = 'anna.vcf'", (days_ago(0),))
    conn.commit()
    conn.close()
    run(db_path)

    assert dtstart(db_path, "anna.vcf") == before + timedelta(days=80)


def test_removing_the_token_deletes_the_nudge_on_the_next_run(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "Met in Palma @d"), days_ago(1))])
    run(db_path)
    assert set(events(db_path)) == {event_uri("anna.vcf")}

    conn = sqlite3.connect(db_path)
    conn.execute("UPDATE cards SET carddata = ? WHERE uri = 'anna.vcf'", (vcard("Anna", "Met in Palma"),))
    conn.commit()
    conn.close()
    run(db_path)

    assert events(db_path) == {}


def test_deleting_a_card_deletes_its_nudge(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(1))])
    run(db_path)

    conn = sqlite3.connect(db_path)
    conn.execute("DELETE FROM cards WHERE uri = 'anna.vcf'")
    conn.commit()
    conn.close()
    run(db_path)

    assert events(db_path) == {}


def test_a_card_with_no_clock_is_skipped_and_counted(db_path, capsys):
    build_db(
        db_path,
        [("anna.vcf", vcard("Anna", "@d"), days_ago(1)), ("ghost.vcf", vcard("Ghost", "@d"), None)],
    )

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf")}
    out = capsys.readouterr()
    assert "Contacts opted in with no lastmodified: 1" in out.err
    assert "from 2 opted-in contacts" in out.out


def test_a_note_with_two_tokens_takes_the_first_and_is_counted(db_path, capsys):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@30d was @90d"), days_ago(0))])

    run(db_path)

    assert dtstart(db_path, "anna.vcf") == today() + timedelta(days=30)
    assert "Contacts with more than one cadence token: 1" in capsys.readouterr().err


def test_every_token_is_stripped_from_the_body(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@30d switched from @90d last year"), days_ago(0))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DESCRIPTION") == "switched from last year"


def test_the_run_reports_opted_in_and_overdue_counts(db_path, capsys):
    build_db(
        db_path,
        [
            ("anna.vcf", vcard("Anna", "@90d"), days_ago(730)),
            ("ben.vcf", vcard("Ben", "@90d"), days_ago(400)),
            ("cleo.vcf", vcard("Cleo", "@90d"), days_ago(1)),
            ("dentist.vcf", vcard("Dentist", "no token"), days_ago(900)),
        ],
    )

    run(db_path)

    assert "Synced 3 nudges from 3 opted-in contacts (2 overdue)" in capsys.readouterr().out


def test_a_regex_matching_nothing_is_distinguishable_from_nothing_being_due(db_path, capsys):
    build_db(db_path, [("dentist.vcf", vcard("Dentist", "Cleaning every spring"), days_ago(900))])

    run(db_path)

    assert "Synced 0 nudges from 0 opted-in contacts (0 overdue)" in capsys.readouterr().out


def test_every_address_book_contributes(db_path):
    build_db(
        db_path,
        [
            ("anna.vcf", vcard("Anna", "@d"), days_ago(1), 1),
            ("shared.vcf", vcard("Shared", "@d"), days_ago(1), 2),
        ],
        addressbooks=(1, 2),
    )

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf"), event_uri("shared.vcf")}


def test_a_blob_stored_card_is_read(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "Met in Palma @d").encode("utf-8"), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DESCRIPTION") == "Met in Palma"


def test_a_latin1_card_does_not_abort_the_run(db_path):
    build_db(
        db_path,
        [
            ("renee.vcf", vcard("Renée", "@d").encode("latin-1"), days_ago(1)),
            ("anna.vcf", vcard("Anna", "@d"), days_ago(1)),
        ],
    )

    run(db_path)

    assert set(events(db_path)) == {event_uri("renee.vcf"), event_uri("anna.vcf")}
    assert field(db_path, "renee.vcf", "SUMMARY") == "Reach out: Renée"


def test_the_run_fails_when_there_is_no_operator_principal(db_path, capsys):
    conn = sqlite3.connect(db_path)
    conn.executescript(SCHEMA)
    conn.execute("INSERT INTO principals (uri) VALUES (?)", (PROXY,))
    conn.commit()
    conn.close()

    sync = nudge.BaikalNudgeSync(db_path)
    try:
        assert sync.sync_nudges() is False
    finally:
        sync.close()

    assert "No operator principal found" in capsys.readouterr().err


def test_dates_are_computed_in_utc_not_the_hosts_local_zone(db_path, monkeypatch):
    """A `VALUE=DATE` event is floating, and `lastmodified` is a UTC timestamp.

    Reading the clock in a local zone would shift the due date by a day against
    Berlin -- noise on a 90-day cadence, but a conversion that looks like a
    correction is how this rots.
    """
    monkeypatch.setenv("TZ", "Pacific/Kiritimati")
    build_db(db_path, [("anna.vcf", vcard("Anna", "@90d"), days_ago(0))])

    run(db_path)

    expected = datetime.now(timezone.utc).date() + timedelta(days=90)
    assert dtstart(db_path, "anna.vcf") == expected


def test_the_uid_is_stable_and_namespaced(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(1))])

    run(db_path)

    uid = field(db_path, "anna.vcf", "UID")
    assert uid.endswith("@baikal-nudge")
    assert re.fullmatch(r"[0-9a-f]{32}@baikal-nudge", str(uid))
    assert nudge.BaikalNudgeSync("")._make_uid("anna.vcf") != nudge.BaikalNudgeSync("")._make_uid("ben.vcf")


def test_an_unchanged_nudge_is_not_rewritten(db_path):
    build_db(db_path, [("anna.vcf", vcard("Anna", "@d"), days_ago(1))])
    run(db_path)
    conn = sqlite3.connect(db_path)
    before = conn.execute("SELECT synctoken FROM calendars").fetchone()[0]
    conn.close()

    run(db_path)

    conn = sqlite3.connect(db_path)
    after = conn.execute("SELECT synctoken FROM calendars").fetchone()[0]
    conn.close()
    assert after == before


LONG_NOTE = (
    "Met in Palma and she runs the co-op down by the harbour which is where we should "
    "meet next time, ask about her sister and the boat"
)


def test_a_token_past_a_fold_still_opts_the_contact_in(db_path):
    """The failure this guards is silent: no event, no stderr line, no count."""
    build_db(db_path, [("anna.vcf", folded_vcard("Anna", f"{LONG_NOTE} @d"), days_ago(10))])

    run(db_path)

    assert set(events(db_path)) == {event_uri("anna.vcf")}
    assert dtstart(db_path, "anna.vcf") == today() + timedelta(days=80)


def test_a_folded_note_reaches_the_body_whole(db_path):
    build_db(db_path, [("anna.vcf", folded_vcard("Anna", f"@d {LONG_NOTE}"), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DESCRIPTION") == LONG_NOTE


def test_a_token_split_across_a_fold_is_read(db_path):
    """`@` ending one line and `30d` opening the next is one token, not none."""
    carddata = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ben\r\nNOTE:Climbing partner @\r\n 30d\r\nEND:VCARD\r\n"
    build_db(db_path, [("ben.vcf", carddata, days_ago(0))])

    run(db_path)

    assert dtstart(db_path, "ben.vcf") == today() + timedelta(days=30)


def test_a_folded_name_reaches_the_summary_whole(db_path):
    name = "Anna Wilhelmina Löw-Vandersteen of the Palma Harbour Cooperative Society"
    build_db(db_path, [("anna.vcf", folded_vcard(name, "@d"), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "SUMMARY") == f"Reach out: {name}"


def test_an_address_split_across_a_fold_is_still_not_a_token(db_path):
    """Unfolding must not manufacture a token out of a wrapped address."""
    carddata = "BEGIN:VCARD\r\nFN:Ben\r\nNOTE:Book the table through anna@\r\n dinner.example\r\nEND:VCARD\r\n"
    build_db(db_path, [("ben.vcf", carddata, days_ago(400))])

    run(db_path)

    assert events(db_path) == {}


def test_indentation_inside_a_note_survives_token_removal(db_path):
    """Only the space the token occupied is collapsed, not the operator's layout."""
    build_db(db_path, [("anna.vcf", vcard("Anna", r"@d Topics:\n  - the boat\n  - her sister"), days_ago(1))])

    run(db_path)

    assert field(db_path, "anna.vcf", "DESCRIPTION") == "Topics:\n  - the boat\n  - her sister"
