# ADR-0007: Auberge owns folder reconcile; expunge and verification stay external

## Status

Accepted, 2026-05-07. Amended 2026-07-29 — see the Amendment section below.

## Decision

Auberge ships `auberge bichon reconcile-folders --host <h>` — an idempotent CLI command (dry-run by default, `--apply` to commit) that reads each Bichon account's live folder list from `GET /api/v1/list-mailboxes/<id>?remote=true`, computes the **Synced Folder** set per the rules below, and PATCHes Bichon's per-account `sync_folders`. **Account creation** (IMAP host, credentials, OAuth2 web-flow consent) remains UI-driven; reconcile is the only state auberge writes to Bichon.

Folder identity is matched primarily by RFC 6154 SPECIAL-USE attributes (`AttributeEnum::{Junk, Trash}` on Bichon's `MailBox` struct — language-portable, hierarchy-portable across `[Gmail]/`, `INBOX.`, etc.), with case-insensitive name matching as fallback for legacy IMAP servers that don't advertise SPECIAL-USE (`Spam`, `Junk`, `Junk Mail`, `Trash`, `Deleted Items`, `Bin`, `Papierkorb`, `Éléments supprimés`, `Pourriels`). The default exclusion set is `{Junk/Spam, Trash}`; everything else — `INBOX`, `Sent`, `Drafts`, `Archive`, and any custom labels — is included. An operator-supplied per-account additive override (`extra_excluded_folders`) is supported in `config.toml`; subtracting from the default exclusions (e.g. archiving Trash anyway) requires a separate explicit flag and is deliberately a foot-gun.

Those two folders are excluded because their contents are already-rejected mail:

- `\Junk` holds messages the provider or user classified as unwanted. Archiving them inflates the corpus and degrades full-text search signal without adding recoverable value.
- `\Trash` holds messages the user already deleted. The **Email Archive** is a durable archive, not a recycle bin; re-ingesting deleted mail contradicts the operator's intent.
- Both turn over at high velocity, adding restic churn for no durable gain.

Auberge **does not** ship tooling for archive verification (coverage-check) or **Upstream Mailbox** expunge. (Amended 2026-07-29: verifying auberge's _own_ offsite snapshots is in scope — see below.) The Email Archive is exposed as a primitive for operators to compose with external tools (`himalaya`, `imap-tools`, Bichon's UI). The `bichon.md` documentation captures the recommended archived-then-expunge workflow, including ordering invariants and a reference shell script at `examples/bichon-expunge.sh`.

This draws the responsibility boundary at a single sentence: **auberge's responsibility ends at "archive is current and backup is current"; the operator's begins at "verify the ordering invariant, then expunge with a tool of their choice."** `himalaya` is the recommended IMAP-side tool (Rust, matches project ethos), but the boundary is what matters, not the tool.

## Why

The deciding principle is **asymmetric automation along the silent-vs-loud failure axis**:

1. **Auberge automates operations whose failures are silent and append-only.** Folder reconcile fits: a wrong tick in Bichon's UI silently archives Spam or Trash forever — the **Email Archive** is append-only by ADR-0006, so a discipline-failure encodes itself permanently into the restic backup corpus. Encoding the policy as code (`exclude SPECIAL-USE Junk/Trash`) eliminates the class of error and makes mistakes recoverable: the next `reconcile --apply` re-converges the configured state.

2. **Auberge does not automate operations whose failures are loud and bounded.** IMAP expunge fits that opposite category: mass mail deletion against the **Upstream Mailbox** is immediately observable, and the human pause between "verify coverage" and "expunge" is precisely the safety mechanism we want — automating it away would normalize destructive operations against third-party state.

3. **Auberge does not own credentials it doesn't need.** Bichon already encrypts IMAP/OAuth2 credentials at rest in its internal store (ADR-0006 references `crates/core/src/users/mod.rs`). Reconcile uses only `bichon_api_token` (already in `config.toml`, already used by `bichon-archive.timer`). Expunge would require IMAP credentials in `config.toml` — duplicating Bichon's encrypted store, expanding blast radius, and (for OAuth2 providers like Gmail) breaking down because the consent flow is fundamentally browser-based.

4. **Selfware ROI for reconcile passes; for expunge/verification it fails on a different axis.** With 7 accounts and rare folder-creation, the _time_ savings of reconcile are negligible — but the _attention budget_ savings are substantial because every UI touch is an opportunity for silent failure. For verification, no comparable attention-budget argument exists: an external script reading auberge's filesystem state is just as reliable as an auberge subcommand reading the same state, because the failure mode of a buggy verifier is loud (mismatch → don't expunge).

## Considered alternatives

- **(α) Auberge owns the full account lifecycle, including IMAP creation and OAuth2.** Rejected. OAuth2 web-flow consent cannot be config-driven; any auberge that owned account creation would have to special-case OAuth providers back to Bichon's UI, creating two onboarding paths and a "fully declarative" claim that is a lie in the most common case. Also duplicates IMAP credentials into `config.toml`, expanding blast radius for zero op-sec gain (Bichon already encrypts them at rest).

- **(γ) Auberge ships `auberge mail coverage-check` (read-only verification) but not expunge.** Rejected after grill-with-docs. The justification "auberge has unique knowledge of the Email Archive and restic snapshot location" collapses on inspection — every required path is filesystem-readable from `config.toml`, and an external script with `find` + `imap-tools` + `restic snapshots` is a well-known ~50-line pattern. The verification tool earned its keep on convenience grounds, not architectural necessity, and convenience-only features have a poor track record of staying small.

- **Strict-β: no reconcile feature, manual UI ticking only.** Rejected on the silent-vs-loud principle. The "30 seconds of UI clicks per account" argument undervalues the attention budget required to avoid silent permanent errors across 7 accounts × N future onboarding events × Bichon UI cosmetic changes over years. Discipline-as-cost is real and asymmetric: a single inattention permanently poisons the archive.

- **Reconcile on a recurring systemd timer** (analogous to `bichon-archive.timer`). Rejected for now. The pain it solves (drift between operator's intent and Bichon's `sync_folders`) does not exist for our actual usage pattern (~1 new folder per year). Timer-based reconcile also forces a premature decision on the apply-vs-report safety model (silent auto-apply normalizes a config typo into weeks of mis-sync; report-only is just systemctl noise). Easy to add later if the manual-trigger model proves insufficient.

- **Reconcile coupled to `auberge ansible run --tags bichon`.** Rejected. First deploy reconciles zero accounts (Bichon has no accounts yet — operator hasn't logged in to add them). Subsequent reconciles only happen on bichon-role redeploys, which have no relationship to folder-creation events. Wrong cadence, awkward bootstrap.

- **Pure name-matching for folder exclusion** (no SPECIAL-USE primary). Rejected. Silently fails on non-English IMAP installs (Corbeille, Müll, Papelera), `[Gmail]/Spam`-style hierarchy prefixes, and case variants. SPECIAL-USE is the IMAP standard for exactly this purpose; using anything else as the primary signal trades portability for nothing.

## Consequences

**Positive:**

- Folder exclusion policy is encoded once in code, not maintained across N sets of UI checkboxes. Discipline-failure mode is eliminated.
- Mistakes are recoverable (re-run `reconcile --apply`) rather than silently permanent (UI-tick → archived forever).
- `auberge bichon reconcile-folders` (dry-run) is a one-command "what is currently being synced?" — auditable system state without trusting Bichon's UI rendering.
- Adding a new account is now: (1) UI: OAuth/IMAP setup; (2) `auberge bichon reconcile-folders --apply`. The second step has one rule and survives Bichon UI redesigns.
- Auberge's mission stays sharp: it owns archive completeness; it does not own destructive operations against third-party services.

**Negative:**

- Operator must remember to invoke `reconcile-folders` after creating an account in Bichon's UI. Mitigated by `bichon.md` docs and (likely) a sentence in the `auberge ansible run --tags bichon` post-task output ("Reminder: run `auberge bichon reconcile-folders --apply` if you've added accounts since the last reconcile.").
- Folder _drift_ (operator creates a new IMAP folder, never re-runs reconcile, new folder isn't in `sync_folders`) is silent until next reconcile. Acceptable: drift in this direction means the new folder isn't archived (loud-ish — the operator notices missing mail in search) rather than the wrong folder _is_ archived (silent, append-only). Direction of failure matters.
- Expunge tooling lives outside auberge — operators who want one-command expunge must either build a wrapper script or accept the two-tool workflow (reconcile + himalaya). Documented as the recommended pattern.

## Amendment (2026-07-29): offsite snapshot verification moves into the binary

`auberge backup verify [-H <host>] [-a <app>] [--max-age <duration>]` ships as a read-only
command: it asserts that a restic snapshot for a Host exists and is younger than a freshness
threshold — with `-a`, the newest snapshot **holding that App's backup**, so one partial sync
cannot false-alarm every other App (#380). Exit `0` verified, `1` a check failed, `2` operational
error, so it composes as a gate in a script.

This does **not** reopen alternative (γ). Coverage-check compares the **Email Archive** against the
**Upstream Mailbox** — two-sided, needs IMAP credentials auberge deliberately doesn't hold, and its
failure mode is loud. That stays out. `backup verify` is one-sided and reads only state auberge
itself produced: its own snapshots, in its own repository, under its own
`…/backups/<host>/<timestamp>` layout. The boundary above is "archive is current and **backup is
current**"; verify makes the second clause executable instead of leaving it as prose an operator
re-derives with `restic snapshots --json | jq` each time.

## Amendment (2026-08-03): the reference script gains an Expunge Sweep

`examples/bichon-expunge.sh --sweep` walks every eligible (account, **Synced Folder**) pair on one
Host with a single window, instead of one operator-chosen pair. The boundary above does not move:
the sweep lives in the reference script, auberge still ships no expunge tooling, and no IMAP
credential enters `config.toml`. Language stays bash for the same reason the script exists at all —
its value is accumulated operator-side hardening (himalaya's `/dev/tty` wizard, `%q`-through-ssh
quoting, pipefail-through-jq), and a rewrite re-litigates that for no functional gain.

What changes is the confirmation granularity, and that is the decision this amendment records. The
single-target script's typed folder name is an intent checksum against a mangled `--folder` flag.
In a sweep no folder is typed — the target set is computed from the Synced Folder set the operator
already curates via **Account Reconcile** — so the risk that checksum defends against does not
exist there, while ~25 per-pair confirmations would create a new one: confirmation fatigue trains
blind typing, which defeats every checkpoint at once. The sweep therefore takes two typed
checkpoints, after every gate has run and one summary table (account × folder × in-window count)
has been printed:

1. **scope** — the operator types the Bichon Host name: binds the sweep to a machine and catches
   the wrong-host class of mistake;
2. **magnitude** — the operator types the grand message total: a number that must be copied from
   the summary, proving it was read.

The hard rules stay: stdin must be a TTY, there is no `--yes`/`--force`, a bare y/N is never a
checkpoint, and `--no-input --sweep` refuses the expunge unconditionally — which makes it a
cron-able fleet-wide coverage verification (all gates, read-only, exit 1 on any finding).

Failure policy: the host-scoped gates (off-host backup, archive freshness) abort the whole sweep —
no pair is safe without them. Per-pair failures skip that pair and continue, classified as benign
(empty window) or finding (coverage gap, pre-existing `\Deleted` mail, unkeyed sidecar, listing
failure, flag-added-but-expunge-failed); every skip is named in the final report with its
remediation line. Exit codes: 0 clean sweep, 1 at least one finding, 2 operational error. A
mid-act failure self-guards: the mail it leaves flagged is exactly what the pre-existing-`\Deleted`
check refuses on the next run, so a partial sweep cannot compound silently.

What changed since (γ) was rejected is the argument, not the convenience. (γ) was refused because
"auberge has unique knowledge" collapsed — every path was filesystem-readable. That still holds for
the archive. It does not hold for the restic repository: the repository URL and password live in
`config.toml` (both supporting `!command` indirection), the host↔snapshot mapping is auberge's own
path convention, and `RESTIC_PASSWORD_COMMAND` must be unset or restic silently resolves the wrong
password. An external script must reimplement all four to be correct, and a verifier that is
_subtly_ wrong is not loud — it prints a green check against the wrong snapshot. That inverts the
silent-vs-loud test that decided this ADR.

Scope is held down by construction: no repository writes, no IMAP, no expunge, no new config keys,
no new dependencies. A snapshot is attributed to a Host by the tag `backup push` writes (#371) —
the same tag `backup prune` groups retention by — with the `…/backups/<host>/<timestamp>` path
covering snapshots pushed before tagging landed.

The binary still ships no expunge tool.

### The reference script may now execute the expunge

`examples/bichon-expunge.sh` (#375) runs `himalaya message expunge` after (a) every gate passes and
(b) the operator types the exact folder name at an interactive terminal. This supersedes the
original script's "print the command, stop short of running it" behavior.

The human pause survives as a mechanism instead of a copy-paste step. There is no `--yes`/`--force`
and no non-interactive execution path: `--no-input` and non-TTY stdin run every gate and then refuse
to expunge. The concern that decided this ADR — "automating expunge on a cron would normalize
destructive operations" — is now enforced structurally rather than by convention, because an
unattended run cannot pass the typed-name gate. The typed name doubles as an intent checksum: a
mangled `FOLDER` variable fails the comparison and aborts. The script remains version-controlled as
a reference and is **not** shipped in the `auberge` binary.

## References

- ADR-0001 — Declarative Backup Recipes. Same principle: state-as-data, not state-as-imperative-code.
- ADR-0006 — Bichon archive feeds Backup Recipe. Establishes the Email Archive as append-only, which is the foundation of the silent-failure asymmetry that motivates this ADR.
- CONTEXT.md — defines **Email Archive**, **Upstream Mailbox**, **Synced Folder**, **Account Reconcile**.
- RFC 6154 — IMAP LIST Extension for Special-Use Mailboxes.
- Bichon API: `POST /api/v1/account/<id>` with `AccountUpdateRequest.sync_folders` (`crates/core/src/account/payload.rs:155`).
- Issue #329 — `auberge bichon reconcile-folders` implementation.
- Issue #374 — `auberge backup verify` offsite snapshot verification.
- Issue #375 — interactive `bichon-expunge.sh` with typed-confirm execute.
