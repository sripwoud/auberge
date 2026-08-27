# ADR-0046: A fence imports the crate it fences

## Status

Accepted, 2026-08-27.

## Decision

The crate ships a library target. Every module lives under `lib.rs` as `pub`; `main.rs` holds the clap tree, the global output flags, the dispatch `match`, and the tests over that clap surface — no business logic.

It follows that **an integration fence asserting about a crate item reaches it with `use`** — never a retyped copy, never the source file read as text. Two rules make that concrete:

- **A closed set has one authority.** systemd's unit types, and anything else the crate declares as the definitive list, are read from the declaration. A fence that needs the set imports `UNIT_TYPE_SUFFIXES`; it does not restate the eleven entries alongside a comment saying where they came from.
- **Source-as-text is for questions about source.** `tests/crate_source/mod.rs` reads `src/**/*.rs` as text because its questions are textual — which modules name a vendor crate, which call sites discard a guard. Recovering one `const`'s _value_ is not such a question, and answering it by string-splitting on Rust syntax is a parser nobody signed up to maintain.

The `pub` surface is deliberately uncurated. Nothing outside this repository consumes the crate, so a narrower export list would be a second interface maintained against Cargo's visibility rules for no reader; the discipline that matters is which vocabulary the fences actually import. Test doubles stay inline under `cfg(test)` — no `test-support` feature until a second consumer exists.

### What it costs

`dead_code` stops reaching the crate. In a bin-only crate every item was reachability-checked; a `pub` item in a `pub` module is a public API, so the lint never fires on it. Measured: deleting the `#[allow(dead_code)]` above `signal::unregister_progress_bar` now produces no warning at all. The ~18 allow sites across 7 modules are inert decoration, and an unused `pub fn` added tomorrow is nobody's warning.

That is a guard that stopped guarding, which is the failure this repo fences against everywhere else, and it is accepted rather than solved: the alternative is the curated surface rejected above, and the lint was catching unused _private_ items — a class the compiler still flags inside each module. Recorded here so the next person to notice the silence finds the decision instead of a bug.

### Not yet universal

Two fences still hand-roll a qualifier, and a _looser_ one — `if name.contains('.')` keeps `foo.bar`, where `qualified_unit_name` yields `foo.bar.service`:

- `tests/install_notifies_restart.rs`
- `tests/service_directories.rs`

Both are among the five fences #668 folds into a shared installed-unit layer, whose settled decision is already "unit vocabulary imported from the lib — no mirror". The rule above is stated in full and those two are the open remainder, named so the gap is not mistaken for coverage.

## Why

Cargo links an integration test against the library target. With none, `tests/` could not `use` anything in the crate, and the two fences over systemd unit names each worked around it:

| Fence                       | Workaround                                                                                                                          |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `unit_ownership`            | retyped the 11-entry unit-type table; reimplemented `qualified_unit_name` as a local `qualified`                                    |
| `removed_unit_failed_state` | retyped the table, then held the copy true by reading `src/playbook_meta.rs` as text and `split_once("];")` to recover the literals |

A mirror is a second authority, and #653 is what that costs: the fence shipped admitting five of systemd's eleven unit types, so a removal of a `.target`, `.slice`, `.swap`, `.mount`, `.path`, or `.socket` failed the suffix test, left the domain without a word, and kept the build green — `test_the_scan_still_sees_every_removal_site` pins only what the scan found, so a scan that found less passed vacuously. #656 patched it by widening the copy and adding the scraper to keep it honest. That is a guard on a guard, and it still leaves the copy as the thing the fence reads.

The import deletes the category rather than the instance. Drift between fence and production is no longer a state the tree can be in, so the test that checked for it loses its subject.

But those mirrors were also an accidental _witness_, and deleting them without noticing that would have traded one silent failure for another. Because each fence held an independent copy, truncating the production const failed their drift check. With the copies gone, dropping a unit type shrinks the qualifier and both fences' domains together — and both fences pass. Measured: with `.automount` removed from `UNIT_TYPE_SUFFIXES`, all 7 tests across `unit_ownership` and `removed_unit_failed_state` go green, which is #653's failure mode restored by the fix for it.

So the drift check is replaced, not merely deleted: `test_the_unit_type_set_is_every_type_systemd_defines` declares systemd's eleven types off `systemd.unit(5)` and matches the const by equality in both directions. That is the ADR-0028 declared regime, and it is not the mirror this ADR forbids — a forbidden mirror restates _this crate's_ table inside a fence, so fence and production can disagree; this restates _systemd's_, which no code here can read. Test count returns to 902.

The split paid a second dividend at no cost. `main.rs` was the crate root, so every `pub(crate)` item was reachable from it; as a separate crate it is not, and the compiler audited the boundary. It found exactly one: `output::set_no_color`, sitting beside `set_verbose` and `set_quiet`, which were already `pub`. That inconsistency had been invisible for as long as there was no boundary to be inconsistent across.

`unit_file_name` moved with the vocabulary it belongs to. It had lived in the backup command, 1,400 lines and a module away from `qualified_unit_name`, both reading the same suffix table — which reads as a duplicate someone forgot to delete. They are intentional forks, and `@` is the whole difference: `systemctl show` addresses the instance (`syncthing@alice.service`), `systemctl list-unit-files` holds only the template (`syncthing@.service`). Picking the wrong one is #619, the restore preflight looking for an instance among unit files. Adjacent, under a shared doc comment naming both questions, the fork is legible.
