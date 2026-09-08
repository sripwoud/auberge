# ADR-0078: Contact cadence lives in the vCard `NOTE`, and the script never reads `CATEGORIES`

## Status

Accepted, 2026-09-08. Closes #845. Extends the birthday sync's model — a generated calendar projected from the address book — to a second kind of event, and adds the shared module both now write through.

## Decision

**The cadence is a token in the contact's `NOTE`, and it is the whole opt-in.** `@d` takes the role default of 90 days; `@30d` or `@180d` override it for that contact. `baikal-nudge-sync.py` reads `NOTE` and nothing else: not `CATEGORIES`, not a group, not an address book, not a config list.

**There is no enable flag.** With no tokens the timer matches zero cards and writes nothing, so the feature is off by being unused rather than by being disabled.

**The token grammar cannot open after a word character, a dot, or an `@`.** That is what keeps `anna@gmail.com` — and `anna@d.example`, which a bare `@(\d*)d\b` matches — out of the grammar.

**The contact clock is `cards.lastmodified`.** sabre/dav stamps it server-side on every PUT, so it reads identically from iOS, Android/DAVx5 or the web admin.

**Overdue contacts clamp to today and resurface on every run.** `max(due, today)`, computed in UTC.

**The timer is weekly.** `OnUnitActiveSec=1w`.

**`TRANSP:TRANSPARENT` on every generated event, and `CLASS:PRIVATE`.** No `VALARM`.

**Both malformed cases skip and count.** A card with a NULL `lastmodified`, and a note carrying two tokens (first wins). Counts go to stderr, following the `Unparsable BDAY values` precedent.

**No CONTEXT.md entry.** `cadence token`, `care calendar` and `contact clock` are documented in `docs/applications/apps/baikal.md`.

## Why

### Why `NOTE` and not `CATEGORIES`

`CATEGORIES` is the field designed for this: a machine-readable, multi-valued list, which is exactly what a cadence marker is. It loses on the only axis that decides whether the feature gets used.

The nudge has to carry _what to talk about_, or it degrades to a recurring alarm labelled with a name — and an alarm you cannot act on is one you learn to dismiss. That text already exists, in `NOTE`, written by hand in a contacts app. Putting the cadence in the same field means one edit in one place sets both the schedule and the payload, and the mobile contact editor puts them next to each other. Splitting them across `NOTE` and `CATEGORIES` means two fields, and on iOS the second one is several taps deeper — enough friction to make a habit not stick.

`NOTE` also degrades better on the client side. Every contacts app on every platform surfaces a Notes field prominently. `CATEGORIES` maps to iOS "groups", which the iOS Contacts app cannot edit at all; setting one requires macOS or DAVx5's raw editor. A grammar only reachable from a desktop is not a grammar for a feature whose whole point is capture on a phone.

The cost is a parser over free text, where `CATEGORIES` would have needed none. That cost is one regex, and it is the regex the negative lookbehind above makes safe.

### Why the token is the toggle

An enable flag would be a second thing to get right, and it can only ever disagree with the tokens. A flag off with tokens present is a silently dead feature; a flag on with no tokens is the same as no flag at all. The `NOTE` scan already answers "does the operator want this", so a flag adds a state where the answer is ambiguous.

The same reasoning excludes an address-book filter: `cards.addressbookid` is ignored, as in the birthday sync. A shared or imported address book contributes nothing unless `@d` was deliberately typed into a note there.

### Why `cards.lastmodified` and not vCard `REV`

`REV` is the field that means "when this card changed", and it is written by the client: formatted differently per client, and omitted entirely by some. `lastmodified` is written by the server on the PUT that carried the change, so it exists for every card and reads the same from every client.

It means **card last modified**, not **person last contacted**. Fixing a phone number resets the clock and the nudge arrives late. Accepted: that degrades to the behaviour before this feature existed, which is no nudge at all.

The honest alternative — a separate "last contacted" field the operator maintains — was rejected because it is the same friction as `CATEGORIES` with an extra failure mode: a field nobody updates makes every contact permanently overdue, which is worse than a clock that is sometimes optimistic.

### Why overdue clamps to today

A card last touched two years ago with `@90d` was due 21 months back. Writing that true date puts the event in a month nobody scrolls to, which fails for exactly the people the feature exists for — the ones who have gone longest without a message.

Clamping makes the nudge resurface on every run until the card is edited. That is the intended behaviour, not churn: the only way to clear it is to actually message the person and let the card's clock move, which is the outcome.

### Why weekly, and why that follows from clamping

Once overdue events clamp, **the run period is the re-nag interval.** Daily re-nagging trains you to swipe past a `Keep in touch` event every morning, and a nudge you have learned to dismiss costs the calendar its credibility for everything else on it.

The cost is that a card you just edited keeps its stale event for up to seven days. That cost is invisible: you already messaged them, so the event is a reminder to do something you have done.

### Why there is no `VALARM`

Clamping puts every overdue contact on the same date, so an alarm is a burst of N notifications the evening before each run — the failure mode above, concentrated. The passive version is the one to try first; adding an alarm later is one line if it proves too quiet.

### Why `TRANSP:TRANSPARENT` is load-bearing

`baikal-busy-sync.py` selects every calendar under the principal with no uri filter, so `care` is swept into the published free/busy feed ([ADR-0010](./0010-baikal-busy-feed-host-sanitized-external-consumer.md)). The `TRANSPARENT` check in `_is_busy` is the only thing excluding these events.

Names do not leak — `SUMMARY:Busy` is hardcoded in the renderer — but an opaque nudge publishes an all-day busy block to anyone holding the feed token on every day you owe someone a message, which under clamping is most days.

The failure is silent and the line looks like decoration, so it is fenced twice: once directly, naming the leak, and once end-to-end against the real busy sync with an overdue contact whose event lands inside the feed's `[now - 1d, now + 60d]` window. Dropping the line fails both.

Filtering `care` out of the busy sync's query instead was rejected: it moves the guard away from the thing being guarded, and it would silently stop protecting the next generated calendar anyone adds.

### Why the dates are UTC

`lastmodified` is a UTC unix timestamp and the event is a floating `VALUE=DATE`. Converting through the host's zone would shift a due date by a day against Berlin — noise on a 90-day cadence, and a conversion that _looks_ like a correction is how this rots into a bug nobody can reason about. Stated in the docs and asserted in a test that runs under `Pacific/Kiritimati`.

### Why `CLASS:PRIVATE`

The birthday builder writes `PUBLIC`, which is right for a birthday. This event body carries the most personal free text in the address book. Nothing reads `CLASS` today, but the delegation it governs is something Baikal supports, so the conservative value is the one to start with.

### Why the note is unescaped before being escaped

vCard stores `NOTE` already escaped in the same TEXT grammar iCalendar uses, so `Ask about the co-op\, then Porto` arrives with literal backslash sequences. Escaping that raw value doubles them and shows the reader a literal `\,`. Unescape-then-escape is close to the identity on well-formed input, and it _normalizes_ a client that left a bare `;` or `,` unescaped — which would otherwise produce an `.ics` sabre/dav stores happily and the client fails to parse. The break is on the read side, so a server-side test cannot see it: the suite asserts the parsed value a client would show, via `icalendar`, not the bytes written.

### Why both malformed cases skip rather than default

`lastmodified` is nullable in the schema. Reading a missing clock as epoch makes the card permanently overdue with no edit able to satisfy it — a nudge that cannot be cleared is the one that gets the whole calendar muted.

A note with `@30d` … `@90d` takes the first match. It is counted, not silently accepted, because the failure is a contact nagging on a cadence you believe you changed.

### Why the prefactor came first

Around 150 of the birthday script's 261 lines were generic: the connection, the calendar row, the uid hash, the principal query, and the insert / update-if-changed / sweep-stale loop. Copying them would have duplicated sabre/dav's synctoken contract, where a divergence is **silent** — clients stop syncing and nothing errors.

`baikal_caldav.py` now holds one copy. Two bugs fell out of that:

- The birthday script took `principals[0]` from a loose `LIKE 'principals/%'`, which can return a `calendar-proxy-read`/`-write` sub-principal that owns no calendars. All three scripts now share busy-sync's strict `NOT LIKE 'principals/%/%'` form.
- `_bump_synctoken` had no coverage at all. Emptying its body left all 61 existing tests green — the exact silent divergence the extraction exists to prevent. Extracting the code did not make it safe; it made it worth fencing, and `test_baikal_caldav.py` now asserts the change row and the token bump for insert, update, delete and no-op.

`test_baikal_birthday_sync.py` is byte-identical across the prefactor. It is the witness that behaviour was preserved, so a rewritten or narrowed suite would have passed vacuously.

### Why no CONTEXT.md entry

Every term there describes the deploy substrate — Host, App, Playbook, Recipe. `cadence token`, `care calendar` and `contact clock` describe one App's data model, and the birthday sync set the precedent by adding none. They live in the App's doc page, and this ADR carries the reasoning. Promote them if a second App ever reads the token.

## What it costs

- **The nudge is late by however long ago the card was last edited for an unrelated reason.** The clock is the card's, not the person's. Documented rather than fixed; the fix is a field nobody maintains.
- **An overdue contact's event moves under you.** The `care` calendar is not a stable record of anything — it is a projection recomputed weekly. Nothing should be scheduled against it.
- **The `care` uri is a stable identifier the script keys on.** Renaming it orphans every event, and the sweep will not find them, because the sweep only sees the calendar it just resolved. Nothing enforces this; it is a comment in the script and a line here.
- **Free-text parsing over the most personal field in the address book.** The regex is guarded against addresses and the tests pin nine negative cases, but a note is written by a human and the grammar is a convention, not a schema.
- **A third weekly timer on a role that had two.** No new subdomain, unit group, or backup path — the data is Baikal's SQLite, already covered by the Recipe's `/opt/baikal/Specific`.
- **`baikal_nudge_default_days` reaches the script as argv, not a Key.** It is deliberately absent from the Key Registry and from `required_keys`: putting it there would make it mandatory fleet-wide for a value one App reads and defaults.

## Alternatives considered

- **`CATEGORIES` for the cadence.** The designed field; rejected on capture friction and on iOS being unable to edit it. See above.
- **A vCard group or a dedicated address book as the opt-in.** Same friction, plus it splits the marker from the payload and makes a shared address book dangerous.
- **A config list of contacts and cadences.** Rejected: it puts the operator's relationships in `config.toml`, needs a redeploy to change, and duplicates identity that already has a canonical home.
- **vCard `REV` as the clock.** Client-written, formatted inconsistently, omitted by some clients.
- **A separate "last contacted" field the operator updates.** The honest clock, and the one nobody maintains. Rejected: an unmaintained field makes every contact permanently overdue.
- **Daily runs.** Rejected: with clamping, the run period is the re-nag interval, and daily trains dismissal.
- **Writing the true overdue date.** Rejected: it buries the event for exactly the people the feature exists for.
- **A `VALARM`.** Rejected for now; it is a burst of N notifications under clamping, and one line to add later.
- **Excluding `care` in the busy sync's query rather than marking events transparent.** Rejected: it moves the guard away from what it guards, and stops covering the next generated calendar.
- **Copying the birthday script instead of extracting.** Rejected: it duplicates the synctoken contract, where divergence is silent.
- **Truncating the note in `DESCRIPTION`.** Rejected: the note is the payload. It is folded per RFC 5545 instead.

## References

- Issue #845 — the request, and the source of the event shape.
- [ADR-0010](./0010-baikal-busy-feed-host-sanitized-external-consumer.md) — the busy feed whose calendar-wide read makes `TRANSP:TRANSPARENT` load-bearing.
- `ansible/roles/baikal/files/baikal_caldav.py` — the shared machinery; `baikal-nudge-sync.py` — the token grammar, the clamp, the builder.
- `ansible/roles/baikal/files/test_baikal_nudge_sync.py` — carries Baikal 0.12.1's `cards` schema verbatim, including the `lastmodified` the birthday fixture omits.
- `docs/applications/apps/baikal.md` — the token grammar and the UTC rule, for the operator.
