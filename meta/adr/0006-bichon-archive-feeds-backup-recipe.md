# ADR-0006: Bichon archive feeds the Backup Recipe; internal store is not backed up

## Status

Accepted, 2026-05-06.

## Decision

Bichon's Backup Recipe rsyncs `/var/lib/bichon-archive/` — a tool-agnostic per-message EML mirror produced hourly by a systemd timer on the Bichon Host — instead of `/opt/bichon/data` (Bichon's encrypted internal store). Restoring Bichon means redeploying the role via `auberge deploy bichon` and re-importing from the EML archive via Bichon's existing import tooling.

The archive is produced by a shell script (`bichon-archive.sh`) deployed by the bichon ansible role. The script walks Bichon's REST API (`/api/v1/current-user`, `/minimal-account-list`, `/accounts/:id/stats`, `/search-messages`, `/download-message/:acc/:msg`), writes one `.eml` per message plus a `.meta.json` sidecar (folder + tags), and is driven by `bichon-archive.timer` on `OnCalendar=hourly` with `RandomizedDelaySec=10min` and `Persistent=true`. State tracking is a per-account cursor file plus a 24-hour overlap window plus skip-if-exists; failures advance no cursor. Concurrency is serial. No new Rust code in `auberge`; the operator-laptop subcommand contemplated in issue #315 is not built.

## Why

Operator priorities are, in order:

1. **Be in control of the data.** Backups must remain readable without Bichon, without its encryption password, and without a working Rust toolchain in the future.
2. **Tool-agnostic format.** Importable into Thunderbird, mutt, or any MBOX/EML-aware client.
3. **Restic-friendly dedup.** Backups should not bloat under per-snapshot churn.

Bichon's internal `/opt/bichon/data` fails all three:

- Its layout is Bichon-specific (Tantivy index + content store, not a tree of `.eml` files).
- The encryption password is permanent and non-rotatable (`crates/core/src/utils/encrypt.rs:32-44`); it wraps stored credentials (`crates/core/src/users/mod.rs:250,420,593,328`) but not message bodies. Losing it locks the operator out of the live install but does not lock them out of message content — except that retrieving message content still requires a working Bichon to call `/download-message`.
- Tantivy index segments rewrite on commit (`store/tantivy/{attachment,envelope}.rs`). Encrypted credential rows likely re-chunk on every change. Both defeat restic's content-defined-chunking dedup.

A write-once tree of `.eml` files is the inverse of all three failure modes: plaintext RFC 5322, no Bichon dependency, perfect dedup across snapshots.

## Considered alternatives

- **Dual channel: keep the existing Backup Recipe AND add a parallel Archive.** Rejected on the same principle as ADR-0001's "no escape hatch from day one." Two pipelines for one App, where one is strictly dominated by the other on every operator-stated priority, is operational debt.
- **Operator-laptop archive over REST (`auberge bichon archive` from the laptop).** Rejected: Bichon was self-hosted precisely because the operator's laptop is not always on; making the _archive cadence_ depend on laptop uptime contradicts that rationale. Also breaks symmetry with the existing `RecipeExecutor` pipeline — every other App's data flows through `rsync_from(host_path, dest)`, so adding a parallel REST-pull path means a permanent special case for one App.
- **New Rust subcommand in `auberge`.** Rejected: a shell script with `curl --retry` and `jq` is sufficient. The whole feature is five well-typed REST calls in a loop. Adding a Rust crate, a build target, and an ansible step to ship the binary to the host buys nothing over a templated `.sh.j2` and `EnvironmentFile=`.
- **Reuse `bichonctl`.** Rejected after source inspection: `bichonctl` is `clap::Parser` with one flag (`--config`) and is fully `dialoguer`-driven (interactive prompts mid-flow, including the export destination — `crates/cli/src/export/mod.rs:37-72`). There is no scriptable mode; expect-driving it would couple the archive to upstream prompt wording.
- **Pure skip-if-exists, no cursor.** Rejected: every run would paginate the entire envelope list to discover what to skip — for a 100k-message mailbox at `page_size=100` that is ~1000 API calls per hourly tick, in steady state, forever.
- **Cursor without overlap window.** Rejected: late-arriving mail (Date header in the past, ingested today) and unverified semantics around whether `/search-messages` filters on RFC 5322 Date or Bichon's ingestion timestamp create silent-miss failure modes. A 24-hour re-examination window costs negligible API calls and is self-healing if the cursor is lost or stale.
- **Templated `bichon-archive@account.service` per account.** Rejected: encodes the account list in two places (Bichon's database AND ansible config). Adding an account in Bichon would require an ansible re-deploy; removing one would leave a stale enabled timer. Bichon's `/minimal-account-list` is the single source of truth; the script enumerates it on every run.

## Consequences

**Positive:**

- Backups survive any future where Bichon stops being maintainable. The encryption password's blast radius shrinks to "the live install only"; no archived corpus depends on it.
- restic dedup degenerates to near-zero new bytes per snapshot in steady state (only newly-arrived `.eml` files are new content).
- Adding Bichon to the existing `Recipe Executor` pipeline is a one-line change to `bichon.meta.yml` (`paths:` value); no new code path in `auberge`.
- The archive is observable: `find /var/lib/bichon-archive -name '*.eml' | wc -l` answers "how many emails do I have backed up?" without any tooling.

**Negative:**

- Recovery from total Host loss is slower than rsync-and-go: re-deploy bichon via ansible, then import the EML archive through Bichon's import tooling. Acceptable trade — the restore is _possible without Bichon at all_ (any IMAP/MBOX-aware client can ingest the EML), which is the point.
- An archive timer that wedges silently (e.g. 401 from a revoked token) lags freshness until the operator notices. Mitigation: `ExecStartPre` token check exits non-zero, surfacing as red in `systemctl list-timers` and in the journal. No data is lost — upstream IMAP retains messages until the next successful run.
- Archive cadence is hourly; up to one hour of mail is not yet in the archive at any given moment. The off-host backup cadence (operator-driven `auberge backup create`) is independent, so the worst-case "lost mail in restic" window is hourly + the time since the last `backup create`.

## References

- Issue #315 — Bichon archive backup automation; closed by this design.
- ADR-0001 — Declarative Backup Recipes. The principle that recipes are _data, not behavior_ is what lets bichon's recipe shift its source path without code changes.
- ADR-0005 — Substrate Apps in `infrastructure.yml`. Same operational philosophy: "host-side state, declared once, runs reliably without operator-laptop coordination."
