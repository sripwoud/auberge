# ADR-0022: The music sync blocklists hidden entries as a class

## Status

Accepted, 2026-08-12.

## Context

`auberge sync music` carried two exclude patterns, `.DS_Store` and `*.tmp`. A Claude Code session run with cwd inside `~/Music` wrote a `.memsearch/` index tree into the library; the sync copied it faithfully, and `/srv/music/.memsearch/` was found on the Host and removed by hand on 2026-08-12 (#492).

That is two defects, not one. The pattern list did not cover the new dropping — and an exclude alone would not have removed the copy already on the Host. rsync's exclude rules are **two-sided by default**: a pattern hides files from the sender's file list _and_ adds an implicit `protect` rule on the receiver, so `--delete` skips whatever it names. Excluding `.memsearch` and nothing more would have frozen `/srv/music/.memsearch` in place permanently.

`.memsearch` is also the third instance of one class. `.DS_Store` (Finder), `.memsearch` (Claude Code), and whatever writes next are all **hidden tool state**. Enumerating instances costs one pattern, one issue, and one release per tool — each added only _after_ that tool's droppings reached Navidrome.

The operator's library decides the shape. Across 2,463 files:

- The only dot-prefixed entries anywhere in it are `.memsearch` and its `.index-state.json`. There is no legitimate hidden content.
- There are no non-hidden droppings at all — `Thumbs.db`, `desktop.ini`, `@eaDir`, `__MACOSX`, `._*` return 0 matches.
- The content is heterogeneous and still growing: 2,301 audio files across five formats, 155 art files, 2 album booklets (`.pdf`), 2 liner-note files (`.txt`) — and, not yet, the `.m3u` **Stations** of ADR-0020. `~/Music` contains no `.m3u` file today.

## Decision

- **The exclusions are a blocklist, never an allowlist.** Everything in the library reaches the Host unless a pattern names it.
- **Hidden entries are excluded as a class**: `--exclude=.*`. A hidden entry in a music library is tool state, never content. It subsumes `.DS_Store` and covers `.memsearch`, `.git`, `.cache`, `.Trash-1000` and the next tool with no edit. Patterns carry no slash, so rsync matches basenames at any depth.
- **`*.tmp` is retained** — a rip/import artifact that is not hidden, so the class rule does not reach it.
- **`--delete-excluded`**, so a match already on the Host is deleted rather than protected. This is what makes the blocklist self-healing: adding a pattern retroactively cleans `/srv/music`, so a dropping that already leaked costs a release rather than a manual SSH.
- **Non-hidden droppings are enumerated if they ever appear.** None has; the list is not pre-populated against hypotheticals.
- Scope is `auberge sync music`. It does not extend to the **Backup Recipe** path, which rsyncs with `-az --relative` and no excludes at all.

## Consequences

**Positive:**

- Three candidate patterns collapse to two, and the hidden class is closed rather than sampled.
- The `.DS_Store` case gains the cleanup it never had: `--delete-excluded` removes existing copies on the next sync.
- Booklets, liner notes, cover art and future `.m3u` **Stations** keep transferring untouched. ADR-0020's "carries m3u for free" property now holds by construction instead of by remembering to name `.m3u`.
- The failure mode stays visible and cheap: an uncovered dropping appears in `/srv/music`, Navidrome ignores what it cannot parse, and one pattern fixes it for every Host retroactively.

**Negative:**

- `--delete-excluded` makes every pattern a deletion authorization. Verified before landing that `/srv/music` held no match, so this change deletes nothing today — but a careless future pattern (`*`) would empty the served library. Review exclude additions as deletions.
- A legitimately hidden file can never be synced. Navidrome's `.ndignore` is the realistic case and is unused on this deployment; the escape hatch is an `--include` ordered ahead of the exclude.
- rsync's in-flight temp files are dot-prefixed (`.big.mp3.I1CKgN`). Measured across a throttled 40 MB transfer: they survive `--exclude=.* --delete-excluded` and the transfer completes intact. Worst case would have cost a retransfer, never data.
- Non-hidden droppings remain uncovered. This is accepted, not solved.

## Alternatives considered

- **Enumerate `.memsearch` beside the other two** — the literal ask in #492. Rejected: correct but recurring. It answers this instance and re-poses the question for the next tool, always after the junk has already reached Navidrome.
- **Allowlist media extensions.** The only option that covers non-hidden droppings too, and rejected on the library's own evidence: an allowlist authored today would contain no `.m3u`, because there is none to observe. The first Station created would silently not reach Liquidsoap, and with `--delete-excluded` would be deleted from the Host if it ever had. Booklets and liner notes go the same way. This is ADR-0020's rejected `radio-*.m3u` convention — "defeated by one omission, and the failure mode is silent" — pointed in the un-publishing direction. The asymmetry decides it: an incomplete blocklist leaves a stray file Navidrome ignores, while a complete allowlist deletes content nobody notices is gone.
- **Hidden directories only (`.*/`), keeping `.DS_Store` explicit.** Would put rsync's temp files structurally beyond `--delete-excluded` instead of relying on measured behavior, but leaves three patterns and still enumerates hidden _files_. Rejected once the temp-file behavior was measured rather than assumed.
- **Per-directory `.rsync-filter` files (`-F`).** Covers hidden and non-hidden alike with no future code change. Rejected: sync policy would leave the repo, invisible to review and to `CONTEXT.md`, and the operator must still notice the dropping first. It saves a release, not the discovery — and release-plz cuts a release per merge.
- **An operator-configurable list in `config.toml`.** Same objection, plus a Key Registry key, validation and docs for something that changes about once a year.
- **Pre-emptively enumerate the non-hidden suspects** (`Thumbs.db`, `desktop.ini`, `@eaDir`, `__MACOSX`, `._*`). Rejected: five patterns for zero observed instances, and still enumeration — merely further ahead of the evidence.
- **Stop the writes instead of filtering them.** Keeping tool state out of `~/Music` is strictly better than filtering downstream and should also happen, but it is orthogonal: it does not clean copies already made, and the sync must be safe against the next tool regardless.

## References

- `src/commands/sync.rs` — `music_rsync_command`; the two patterns and `--delete-excluded`.
- ADR-0020 — **Stations** depend on the blocklist carrying `.m3u` without naming it.
- #492.
