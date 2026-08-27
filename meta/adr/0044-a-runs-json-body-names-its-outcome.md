# ADR-0044: A run's JSON body names its outcome and carries its plan

## Status

Accepted, 2026-08-27. **Extends ADR-0004's stdout contract for `dns set-all`**: `--output json` emits a body on every path the run can end on, not only the paths that wrote.

## Decision

`dns set-all --output json` emits one body shape on every path:

```json
{
  "outcome": "applied" | "dry_run" | "cancelled",
  "planned": [{ "app": "...", "subdomain": "...", "fqdn": "...", "ip": "..." }],
  "created": [...],
  "skipped": [...],
  "failed": [...]
}
```

Three rules govern the shape:

- **`outcome` is the discriminant.** `created` is empty on a dry run, a cancelled run, and an all-failed run alike, so emptiness decides nothing; the consumer branches on `outcome` without reading stderr. The three states are mutually exclusive, so they are one enum field — a `dry_run`/`cancelled` boolean pair would make the illegal both-true state representable.
- **`planned` is the denominator, on every outcome.** It holds the full plan — app, subdomain, fqdn, and the effective IP after per-app overrides. On `applied`, `created` and `failed` partition it, so a fail-fast stop is reconcilable structurally instead of by knowing that abandoned records ride in `failed` under a synthetic error. On `dry_run` and `cancelled` it is the primary data, and its records appear nowhere else because nothing was attempted.
- **An array never lies.** `created` holds only records that were created. A dry run's plan does not impersonate a result.

An empty plan is not a fourth state. The run short-circuits before the prompt, so it reports `dry_run` when `--dry-run` asked for a preview and otherwise `applied` — vacuously: `created` and `failed` partition an empty `planned`. `cancelled` is unreachable there because nothing was put to the operator to decline.

Exit codes are unchanged: a dry run and a cancelled run still exit 0, per the documented contract. The body, not the code, says which zero happened.

## Why

Three `--output json` paths returned with empty stdout: `--dry-run`, a declined `Proceed?`, and — until #675 — a failed write. ADR-0004's contract is that `--output json` puts the command's primary data on stdout, and a dry run's primary data is the plan, which `plan_set_all` computes in full and which reached only stderr as human chrome.

The cancelled path is sharper than it looks: `prompt::confirm` returns `false` on non-TTY stdin, so a JSON consumer that forgets `--yes` lands exactly there — empty stdout, exit 0, indistinguishable from a run with nothing to do. With the body, that consumer reads `"outcome": "cancelled"` and a non-empty `planned`, which is precisely the "you forgot `--yes`" signal.

## Alternatives considered

- **The plan under `created` with `dry_run: true` and `success: false` throughout** (issue #678's second candidate). Rejected: `created` would hold records that were not created, and every consumer that misses the flag misreads a preview as a real run. `success: false` on records that did not fail compounds the lie.
- **`planned` populated only on a dry run.** Rejected: the schema becomes mode-dependent, and an empty-plan dry run is undecidable — `planned: []` could mean "not a dry run" or "a dry run with nothing to do". The always-populated array costs a few duplicate rows on applied runs and buys one uniform schema.
- **A `cancelled`/`dry_run` boolean pair instead of the enum.** Rejected: two booleans encode four states for three meanings.
