# ADR-0011: Exclude `rust/cleartext-logging` repo-wide via advanced CodeQL setup

## Status

Accepted, 2026-07-19.

## Context

CodeQL default setup runs `rust/cleartext-logging`, which treats every `println!`/`writeln!` as a log sink. auberge is a single-operator personal-server CLI: stdout is the operator's TTY, not a log file or aggregation pipeline. Whenever a command's data output flows from an email-shaped or otherwise heuristically-tagged field, the query fires a false positive.

Every `println!` in the command modules is intended data output, blessed by ADR-0004's stdout discipline (_"Bare `println!` in command modules is acceptable only for the command's data output"_). The two strongest instances a reviewer would flag:

- `src/commands/bichon/reconcile.rs` prints `account.email` — the account identifier the folder diff is grouped by (PR #331; alerts #16–#21, dismissed as false positives).
- `src/commands/headscale.rs` prints a Tailscale pre-auth key (`key.key`) — the entire purpose of `create-authkey`. The key goes to stdout (data), enrollment instructions to stderr (chrome). An operator captures it with `authkey=$(auberge headscale ... create-authkey)`.

The query's risk model — a secret silently reaching a persisted or shipped log — does not apply: the repo has zero `tracing`/`log`/`env_logger`/`slog` dependencies and no file or log sink on any output path. Every byte is deliberately handed to the operator's terminal.

## Decision

Migrate code scanning from default setup to advanced setup and exclude the single false-positive query repo-wide.

`.github/codeql/codeql-config.yml`:

```yaml
name: auberge CodeQL config
query-filters:
  - exclude:
      id: rust/cleartext-logging
```

`.github/workflows/codeql.yml` invokes `github/codeql-action/init@v4` with `build-mode: none` and `config-file`, on push/PR to `master` plus a weekly schedule. Its matrix covers `actions`, `python`, and `rust` — the exact languages default setup scanned — so the migration drops no language coverage (Python includes security-sensitive Ansible helpers such as `colporteur/files/rewrite-passwords.py`).

Default setup must be disabled in Settings → Security → Code scanning before the advanced workflow can upload results; the two configurations cannot coexist.

## Consequences

**Positive:**

- Ends the recurring false-positive dismissal toil (#332). New CLI commands that print account context no longer trip the query.
- Surgical to the query: every other CodeQL query stays active repo-wide — command injection, path traversal, etc. remain enforced on the CLI layer.
- The "why is a security query off?" question is answered in-repo alongside the other ADRs, not in a closed issue.

**Negative:**

- One-way-ish migration off default setup; requires a manual Security-tab step plus two committed YAML files.
- If auberge ever adds a real log framework (`tracing`, file sink), `cleartext-logging` will no longer catch secrets reaching it. No mitigation needed today (no such framework); revisit this ADR if one is introduced.

## Alternatives considered

- **Per-path suppression** (issue #332 Option 1 — exclude the query only on `src/output.rs` + `src/commands/**`). Rejected: technically impossible. CodeQL `query-filters` filter by query metadata (`id`, `tags`, `problem.severity`) only; there is no `paths` key, and path filtering (`paths`/`paths-ignore`) does not intersect with query selection ([github/codeql#16106](https://github.com/github/codeql/issues/16106)).
- **`paths-ignore` the CLI layer** (issue #332 Option 2). Rejected: removes _all_ queries from those files, losing command-injection / path-traversal detection across the entire CLI surface — broader blast radius than excluding one query.
- **Fix the code instead of the config.** Rejected: there is no defective pattern. Redacting the value destroys the feature; routing it to stderr violates ADR-0004 (data belongs on stdout, pipeable); an `OutputSink` abstraction is churn with no product value and does not change the value→sink dataflow the query matches.
- **Keep default setup and dismiss alerts by hand.** Rejected: that is the status quo this ADR exists to end.

## Implementation notes

- Omitting the `queries:` key keeps the default query suite, matching prior default-setup behavior and minimizing alert churn.
- Stale alerts #10/#11/#14/#15 referenced `src/output.rs` lines that were refactored away (commit `e6d32cd`); they auto-close on the next scan.
- The CI `check-stdout-discipline.sh` guard is orthogonal: it enforces the stdout=data / stderr=chrome routing (correctness), while this ADR addresses the security-query false positive. Together they codify ADR-0004's stdout discipline in CI.
