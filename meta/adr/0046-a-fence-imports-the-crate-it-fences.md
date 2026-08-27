# ADR-0046: A fence imports the crate it fences

## Status

Accepted, 2026-08-27.

## Decision

The crate ships a library target. Every module lives under `lib.rs` as `pub`; `main.rs` holds the clap tree, the global output flags, and the dispatch `match`, and nothing else.

It follows that **an integration fence asserting about a crate item reaches it with `use`** — never a retyped copy, never the source file read as text. Two rules make that concrete:

- **A closed set has one authority.** systemd's unit types, and anything else the crate declares as the definitive list, are read from the declaration. A fence that needs the set imports `UNIT_TYPE_SUFFIXES`; it does not restate the eleven entries alongside a comment saying where they came from.
- **Source-as-text is for questions about source.** `tests/crate_source/mod.rs` reads `src/**/*.rs` as text because its questions are textual — which modules name a vendor crate, which call sites discard a guard. Recovering one `const`'s _value_ is not such a question, and answering it by string-splitting on Rust syntax is a parser nobody signed up to maintain.

The `pub` surface is deliberately uncurated. Nothing outside this repository consumes the crate, so a narrower export list would be a second interface maintained against Cargo's visibility rules for no reader; the discipline that matters is which vocabulary the fences actually import. Test doubles stay inline under `cfg(test)` — no `test-support` feature until a second consumer exists.

## Why

Cargo links an integration test against the library target. With none, `tests/` could not `use` anything in the crate, and the two fences over systemd unit names each worked around it:

| Fence                       | Workaround                                                                                                                          |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `unit_ownership`            | retyped the 11-entry unit-type table; reimplemented `qualified_unit_name` as a local `qualified`                                    |
| `removed_unit_failed_state` | retyped the table, then held the copy true by reading `src/playbook_meta.rs` as text and `split_once("];")` to recover the literals |

A mirror is a second authority, and #653 is what that costs: the fence shipped admitting five of systemd's eleven unit types, so a removal of a `.target`, `.slice`, `.swap`, `.mount`, `.path`, or `.socket` failed the suffix test, left the domain without a word, and kept the build green — `test_the_scan_still_sees_every_removal_site` pins only what the scan found, so a scan that found less passed vacuously. #656 patched it by widening the copy and adding the scraper to keep it honest. That is a guard on a guard, and it still leaves the copy as the thing the fence reads.

The import deletes the category rather than the instance. Drift between fence and production is no longer a state the tree can be in, so the test that checked for it loses its subject and goes with it — 902 tests to 901, and the one that left was the one whose failure mode is now unrepresentable.

The split paid a second dividend at no cost. `main.rs` was the crate root, so every `pub(crate)` item was reachable from it; as a separate crate it is not, and the compiler audited the boundary. It found exactly one: `output::set_no_color`, sitting beside `set_verbose` and `set_quiet`, which were already `pub`. That inconsistency had been invisible for as long as there was no boundary to be inconsistent across.

`unit_file_name` moved with the vocabulary it belongs to. It had lived in the backup command, 1,400 lines and a module away from `qualified_unit_name`, both reading the same suffix table — which reads as a duplicate someone forgot to delete. They are intentional forks, and `@` is the whole difference: `systemctl show` addresses the instance (`syncthing@alice.service`), `systemctl list-unit-files` holds only the template (`syncthing@.service`). Picking the wrong one is #619, the restore preflight looking for an instance among unit files. Adjacent, under a shared doc comment naming both questions, the fork is legible.
