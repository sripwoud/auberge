# ADR-0007: Auberge owns folder reconcile; expunge and verification stay external

## Status

Accepted, 2026-05-07.

## Decision

Auberge ships `auberge bichon reconcile-folders --host <h>` — an idempotent CLI command (dry-run by default, `--apply` to commit) that reads each Bichon account's live folder list from `GET /api/v1/list-mailboxes/<id>?remote=true`, computes the **Synced Folder** set per the rules below, and PATCHes Bichon's per-account `sync_folders`. **Account creation** (IMAP host, credentials, OAuth2 web-flow consent) remains UI-driven; reconcile is the only state auberge writes to Bichon.

Folder identity is matched primarily by RFC 6154 SPECIAL-USE attributes (`AttributeEnum::{Junk, Trash}` on Bichon's `MailBox` struct — language-portable, hierarchy-portable across `[Gmail]/`, `INBOX.`, etc.), with case-insensitive name matching as fallback for legacy IMAP servers that don't advertise SPECIAL-USE. The default exclusion set is `{Junk/Spam, Trash}`. An operator-supplied per-account additive override (`extra_excluded_folders`) is supported in `config.toml`; subtracting from the default exclusions (e.g. archiving Trash anyway) requires a separate explicit flag and is deliberately a foot-gun.

Auberge **does not** ship tooling for archive verification (coverage-check) or **Upstream Mailbox** expunge. The Email Archive is exposed as a primitive for operators to compose with external tools (`himalaya`, `imap-tools`, Bichon's UI). The `bichon.md` documentation captures the recommended archived-then-expunge workflow, including ordering invariants and a reference shell script at `examples/bichon-expunge.sh`.

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

## References

- ADR-0001 — Declarative Backup Recipes. Same principle: state-as-data, not state-as-imperative-code.
- ADR-0006 — Bichon archive feeds Backup Recipe. Establishes the Email Archive as append-only, which is the foundation of the silent-failure asymmetry that motivates this ADR.
- CONTEXT.md — defines **Email Archive**, **Upstream Mailbox**, **Synced Folder**, **Account Reconcile**.
- RFC 6154 — IMAP LIST Extension for Special-Use Mailboxes.
- Bichon API: `POST /api/v1/account/<id>` with `AccountUpdateRequest.sync_folders` (`crates/core/src/account/payload.rs:155`).
