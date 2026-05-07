# ADR-0007: Bichon folder reconcile scope and silent-vs-loud principle

## Status

Accepted, 2026-05-07.

## Decision

### Folder reconcile scope

The `auberge bichon reconcile-folders` command (issue #329) will, by default,
tick every IMAP folder **except** those bearing RFC 6154 `SPECIAL-USE`
attributes `\Junk` and `\Trash`. In practice this means excluding any folder
whose name matches the provider's local equivalent: `Spam`, `Junk`, `Junk
Mail`, `Trash`, `Deleted Items`, `Bin`, `Papierkorb`, `Éléments supprimés`,
`Pourriels`, etc. The rule is attribute-first (the IMAP `LIST` response's
`\Junk` / `\Trash` flag); name-matching is a fall-back for servers that omit
the attribute.

**Why exclude junk and trash?**

- `\Junk` folders contain messages the provider or end-user has already
  classified as unwanted. Archiving them inflates the corpus and degrades
  full-text search signal without adding recoverable value.
- `\Trash` folders contain messages the end-user has already deleted. Bichon
  is a durable archive, not a recycle bin. Re-ingesting deleted messages
  contradicts the operator's intent.
- Both folders turn over at high velocity, adding restic churn for no durable
  gain.

All other standard folders (`INBOX`, `Sent`, `Drafts`, `Archive`, and any
custom labels/folders) are included by default.

Until `reconcile-folders` ships, operators must tick all folders in the Bichon
UI **except** `Spam`/`Junk`/`Junk Mail` and `Trash`/`Deleted Items` (and
their local-language equivalents).

### Silent-vs-loud principle

Auberge does **not** ship an expunge tool. The decision to permanently delete
messages from the **Upstream Mailbox** is a destructive, operator-owned action
that must remain explicit and deliberate. Shipping an expunge command — or
automating expunge on a cron — would make data loss too easy.

The design is:

- Auberge's responsibility ends at "archive is current and backup is current."
- The operator's responsibility begins at "verify the ordering invariant, then
  expunge with a tool of their choice."
- Recommended tooling: `himalaya` (Rust, matches project ethos) for IMAP-side
  ops.

This split — Auberge is silent on destructive IMAP operations — is the
**silent-vs-loud principle**: the system is loud (logs, timer status,
`journalctl`) about ingestion and archiving, but deliberately silent about
anything that deletes from the authoritative upstream source.

## Why

1. **Correctness of scope.** A reconcile command that includes `\Trash` would
   ingest messages the operator has already decided to discard, polluting the
   archive.
2. **Safety of expunge.** The one-way nature of expunge means a bug or
   misconfiguration in an automated expunge path has no recovery — the mail is
   gone from the upstream server. A human pause between "archive confirmed"
   and "expunge" is the only reliable safety net.
3. **Tool autonomy.** Different operators use different IMAP clients.
   `himalaya`, `mbsync`, `mu4e`, Thunderbird, and custom scripts are all valid
   expunge surfaces. Auberge shipping one would either create a preference
   conflict or require supporting multiple.

## Considered alternatives

- **Ship `auberge bichon expunge`.** Rejected: see silent-vs-loud principle
  above. A reference shell script (`examples/bichon-expunge.sh`) documents
  the workflow without putting it in the binary.
- **Default to include all folders (no exclusion).** Rejected: `\Trash` and
  `\Junk` have unambiguous semantics; defaulting to include them is surprising
  and immediately wrong for every real-world account.
- **Automate expunge on a cron after backup succeeds.** Rejected: removes the
  human pause; a single mis-timed run (archive incomplete, backup not yet
  synced) would cause silent data loss.

## Consequences

- Operators must verify `sync_folders` in Bichon's UI before expunging any
  folder. A folder that was unticked in Bichon will not be in the archive; see
  the foot-gun warning in the operator playbook.
- `reconcile-folders` (issue #329) is the long-term fix for the manual
  folder-tick step. Until it ships, the manual step is documented in
  `docs/applications/apps/bichon.md`.
- `examples/bichon-expunge.sh` is version-controlled as a reference but is
  **not** shipped in the `auberge` binary.

## References

- Issue #329 — `auberge bichon reconcile-folders` feature implementation.
- ADR-0006 — Email Archive feeds Backup Recipe; append-only invariant.
- CONTEXT.md — **Email Archive**, **Upstream Mailbox**, **Synced Folder**.
- RFC 6154 — IMAP LIST Extension for Special-Use Mailboxes.
