# ADR-0069: A Host's roster entry preserves fields it does not recognize

## Status

Accepted, 2026-09-01.

## Decision

**`Host` (`src/hosts.rs`) carries a `#[serde(flatten)] unknown: toml::Table` field.** Any `hosts.toml` key under a `[[hosts]]` entry that the compiled binary's `Host` struct does not declare is captured into this map on load and re-emitted verbatim on save, instead of being silently discarded.

Every call site that reconstructs a `Host` field-by-field (`commands::host::run_host_add`, `run_host_edit`, and every test fixture) sets this field explicitly — a fresh host gets an empty table, and `run_host_edit`, the one production site that already holds an existing Host, threads `host.unknown` through unchanged, the same way it already threads `python_interpreter`/`become_method`/`tailscale_ip` past prompts that never ask about them.

## Why

Any mutating `host` subcommand loads the whole roster, mutates the struct in memory, and writes the whole file back. A binary that predates a field previously deleted it silently on that round trip — reading succeeded, so nothing warned. `tailnet_tag` (#767) was lost this way once; `prefer_tailnet` (#787) would make the next loss a silent route change rather than a missing label, because under that issue's strict-no-fallback rule a Host that reverted to its public address raises no error.

The mise pin routinely lags master, so an older `auberge` on `PATH` reading a newer `hosts.toml` is the normal state, not an edge case — the fix has to make the _old_ binary harmless, not just warn a _new_ one.

### Why round-trip rather than refuse or version

Two other shapes were on the table (#788):

- **Refuse to write when the parse saw keys the binary does not know.** Turns every lagging-binary encounter into a hard failure — worse than today's silent loss for the common case (an operator running `host list` on an old binary should not be blocked by a field a newer machine added).
- **A schema version in `hosts.toml` that a too-old binary refuses to write.** Solves the same problem with a second, hand-maintained number to keep in sync with every field addition, and still refuses rather than degrades.

Round-tripping needs neither: an old binary keeps working, a field it cannot interpret survives anyway, and nothing needs updating when a new field is added — the `flatten` map absorbs it by construction.

## What it costs

- `Host` literals across the crate (~12 sites: production `add`/`edit`, and fixtures in `hosts.rs`, `services::inventory`, `services::ssh`, `services::ssh_include`, `commands::backup`, `commands::headscale`) each gained one field. Mechanical, and `cargo check` catches an omission immediately — the type has no `Default`.
- `run_host_edit` is the one production site holding a real, already-loaded `Host`; forgetting to carry `unknown` forward there would silently reintroduce exactly the bug this ADR closes, for any field this binary cannot ask about through a prompt. No fence catches that class of regression short of reading the literal itself — the round-trip test below exercises `HostManager::save_hosts`/`load_hosts` directly, not the edit command's reconstruction.

## Alternatives considered

- **Preserve at the `HostsConfig` level instead of per-`Host`.** Rejected: every concrete regression named in #788 (`tailnet_tag`, `prefer_tailnet`) is a field _within_ a `[[hosts]]` entry, not a top-level roster key — flattening at the wrong level would not have caught either.
- **A hand-maintained list of known field names, checked in `save_hosts`.** Rejected before writing: it recreates the exact failure mode this ADR closes — a maintainer adds a field to `Host` and forgets to add it to the list, and the list (unlike `serde(flatten)`) has no compiler check tying it to the struct's actual fields.

## References

- Issue #788 — the reported bug and its three-option triage.
- Issue #767 — `tailnet_tag`, the field this has already happened to once.
- Issue #787 — `prefer_tailnet`, the field this closes the gap ahead of.
- `src/hosts.rs` — `Host::unknown`, `an_unrecognized_key_survives_a_load_and_save_round_trip`.
- CONTEXT.md — **Host**.
