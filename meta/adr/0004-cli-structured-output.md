# ADR-0004: CLI structured output is `--output {human, json}`, only on commands with load-bearing JSON

## Status

Accepted, 2026-05-05.

## Context

Issue #294 surfaced an uneven CLI output surface: `dns list` carried a tri-valued `OutputFormat::{Human, Json, Tsv}` enum, `dns set-all` accepted but ignored an `--output` argument, `backup list` had its own `{Table, Json, Yaml}`, and most other commands produced only prose. CI scripts asserting deploy outcomes were forced to grep human-formatted lines.

The issue cited [clig.dev](https://clig.dev): _"support `--json` for structured output and `--plain` for scriptable tabular data."_ A reflexive reading suggested adding three formats (human, JSON, TSV) to every list-like and operation-result command across the CLI — and PR #297 implemented exactly that across ~10 commands.

Two design questions surfaced under review:

1. **Does TSV earn its keep?** No named consumer (CI, scripts, teammate pipelines, personal `awk` habits) existed for any TSV output in the project. The format had landed in `dns list`'s original enum and `backup list`'s YAML on the same speculative-symmetry impulse. TSV's awkwardness was concrete: nested commands like `dns status` (with `active_a_records: [...]`) flatten lossily to a 1-row summary; single-record commands like `dns set` produce a header + 1 row that no script would prefer over JSON.
2. **Does every command need `--output`?** Some commands' JSON shape echoes back the caller's own input (`dns set` returns `{subdomain, fqdn, ip, production}` — all derivable from the args). Adding a flag there is the same speculative-symmetry trap: a code path with no consumer the issue can name.

## Decision

- The shared output type is `OutputFormat { Human, Json }` — two values, no TSV, no YAML.
- The flag is `-o, --output {human, json}` (default `human`), exposed as a clap `value_enum`. Not a boolean `--json`.
- The flag is added _only_ to commands whose JSON output contains at least one **load-bearing** field — i.e. a value the caller could not have predicted before the call. Commands whose JSON would merely echo their input arguments do not get the flag.
- For every command with `--output`:
  - JSON output goes to **stdout** unconditionally on `--output json`. `--quiet` does not suppress it.
  - Human output goes to **stderr** for chrome (banners, progress, info messages) and to stdout for the data itself (tables). `--quiet` may suppress chrome on `human` only.
  - No `println!` of info/warn/error chatter on any format; chrome is `eprintln!`.

### Commands that get `--output`

| Command                               | Load-bearing field                                              |
| ------------------------------------- | --------------------------------------------------------------- |
| `dns list`                            | the records themselves                                          |
| `dns status`                          | `missing_subdomains`, `active_a_records`                        |
| `dns migrate`                         | per-record `success`                                            |
| `dns set-all`                         | per-record `success`, `error`                                   |
| `dns delete`                          | `deleted: bool` (idempotency: was it a real delete or a no-op?) |
| `host list`                           | the host inventory                                              |
| `headscale list-users` / `list-nodes` | upstream JSON pass-through                                      |
| `backup list`                         | the backup inventory                                            |

### Commands that do _not_ get `--output`

| Command     | Why not                                                                                                                |
| ----------- | ---------------------------------------------------------------------------------------------------------------------- |
| `dns set`   | All output fields (`subdomain`, `fqdn`, `ip`, `production`) are derivable from args; exit code carries success/failure |
| `host show` | Nested config dump; same data is reachable via `host list --output json \| jq '.[] \| select(.name=="X")'`             |

## Consequences

**Positive:**

- One rule decides whether a future command gets `--output`: _is at least one JSON field load-bearing?_ Removes the speculative-symmetry temptation that drove the original sprawl.
- TSV/YAML branches deleted across `dns list`, `dns set-all`, `backup list`, `host list`, `host show`, `headscale list-*`. Smaller diff, fewer code paths to maintain, no lossy flattening of nested data.
- The shared `OutputFormat` type stays in `src/output.rs` as one source of truth.

**Negative:**

- Breaking: `dns list --output tsv`, `backup list --format yaml` (existing) and `backup list --format` (renamed to `--output`) are gone. Mitigated by the no-consumer test that informed the decision.
- Asymmetric surface: `dns set` lacks `--output` while `dns delete` has it. Future contributors must apply the load-bearing-field rule rather than reflexively copying.

## Alternatives considered

- **Boolean `--json` flag** (clig.dev's literal phrasing). Pros: smallest API; matches the cited document verbatim; matches the `--no-color` precedent (#298). Rejected: the codebase already had `--output` enums in `dns list` and `backup list`; switching to boolean is a separate refactor without a concrete payoff; forward-compatibility (adding a third format if a real consumer ever materializes) is additive with an enum, breaking with a boolean.
- **Three-format enum `{Human, Json, Tsv}` everywhere** (PR #297 as initially submitted). Rejected: TSV had no named consumer and produced lossy output for nested data (`dns status`, `host show`).
- **`--output` on every command, including `dns set` / `host show`**. Rejected: same speculative-symmetry smell as TSV, one level up. The load-bearing-field rule defends the boundary.
- **Keep TSV on truly tabular list commands only** (`dns list`, `host list`, `headscale list-*`, `backup list`); drop on single-record/nested commands. Rejected for now: still no named consumer; can be added back with a real consumer story without breaking change.

## Implementation notes

### The load-bearing-field rule

A field is load-bearing if its value is not derivable from the caller's inputs at the moment of the call. Examples:

- `dns set` → `{subdomain, fqdn, ip, production}` — every field is input-echo. **Not load-bearing.**
- `dns delete` → `{deleted, fqdn, production}` — `deleted` answers "real delete or idempotent no-op?", which the caller does _not_ know. **Load-bearing.**
- `dns set-all` → `[{subdomain, success, error?}]` — per-subdomain outcome is the whole point. **Load-bearing.**

Apply the rule when proposing `--output` on a new command. The conversation lives in this ADR; the heuristic survives the conversation.

### Stdout discipline

`output::info` / `success` / `warn` already use `eprintln!`. Bare `println!` in command modules is acceptable only for the command's data output. Audit scope for this ADR: the commands listed above. CLI-wide audit (e.g. `host remove`'s "Cancelled.", `ssh`'s "Cancelled") is tracked separately in #302.

### `--quiet` stays orthogonal to `--output`

`Cli::quiet` was parsed but never wired to `output::set_*` — the flag existed on the parser but was dropped on the floor (the gap #294 surfaced). That is resolved: `output::set_quiet(cli.quiet)` now suppresses `output::info/success/warn` on the `human` format (PR #306, closes #301).

`--quiet` is _not_ folded into `OutputFormat` as a third variant. The two flags have different scopes:

- `--output` is per-command, data-shape, defined only on commands with load-bearing JSON.
- `--quiet` is global, verbosity, meaningful on every chrome-emitting command including those without structured output (`auberge deploy --quiet`, `auberge ansible run --quiet`).

Folding `quiet` into `--output` would either strip `--quiet` from non-output commands or duplicate the concept across two surfaces. Neither pays off. Wiring `--quiet` to suppress `output::info/success/warn` on the `human` format landed in #306.
