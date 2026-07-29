# ADR-0012: The Email Archive splits immutable bodies from mutable metadata

## Status

Accepted, 2026-07-29.

## Decision

The **Email Archive** keeps its existing on-disk layout. Tags move out of the per-message sidecar into a per-account **Tag Snapshot**, and restore becomes a script instead of an appeal to Bichon's importer.

```
/var/lib/bichon-archive/<account>/YYYY/MM/<envelope-id>.eml         unchanged
/var/lib/bichon-archive/<account>/YYYY/MM/<envelope-id>.meta.json   {folder}
/var/lib/bichon-archive/<account>/tags.json                         {message-id: [tags]}
```

- Layout is **unchanged**. No migration, no file moves, no re-upload.
- `.eml` files stay byte-exact copies of what `/download-message` returns. No header is injected.
- The sidecar stops recording `tags` going forward. `folder` — already written for every message archived to date — stays and becomes the authoritative folder record. Existing sidecars are not rewritten; their vestigial empty `tags` field is ignored on read.
- `tags.json` maps RFC 5322 `Message-ID` → tags and is **rewritten in full on every run**. It is built from `GET /api/v1/all-tags`, then one `POST /api/v1/search-messages` per tag (`EmailSearchFilter.tags`). When `all-tags` returns `[]` the run stops after one call.
- `examples/bichon-restore.sh` performs restore non-interactively: group `.eml` files by their sidecar's `folder`, `POST /api/v1/import` per folder, then replay `tags.json` through `POST /api/v1/update-tags`. Reference script, version-controlled, not shipped in the `auberge` binary — the same standing as `examples/bichon-expunge.sh`.

## Why

ADR-0006 declared the restore path — "re-importing from the EML archive via Bichon's existing import tooling" — without exercising it. Tracing it end to end shows it recovers bodies only:

| Artifact            | Recovered by ADR-0006's path as designed | Why                                                                                                                                                                                          |
| ------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Bodies, attachments | yes                                      | plaintext RFC 5322, imported verbatim                                                                                                                                                        |
| Folder              | **no**                                   | `bichonctl` derives `mail_folder` from the _full_ relative parent path (`crates/cli/src/eml/mod.rs:84-88`); against a date-partitioned archive that yields folder names like `INBOX/2026/07` |
| Tags                | **no**                                   | the sidecar's JSON shape is read by no Bichon tool                                                                                                                                           |

Fixing folder is not a path change, because folder-in-path and date-partitioning cannot coexist: `scan_dir` treats every directory level below the import root as part of the folder name, so any date partition becomes folder-name suffix. Choosing folder-in-path would mean abandoning date partitioning and putting every message of a folder in one directory.

Fixing tags is not a transport problem either, because tags are a different _kind_ of value:

1. `Envelope.tags` is populated **only** from an `X-Bichon-Metadata` header (`crates/core/src/envelope/extractor.rs:165-186,245`). No IMAP flag, keyword, or Gmail-label mapping exists anywhere in Bichon, so mail arriving over IMAP always has `tags: None`.
2. Tags therefore exist only by manual application through `TagsRequest` (`crates/core/src/message/tags.rs:26`) — which happens _after_ ingestion, and is revised indefinitely thereafter.
3. The Archive's skip-if-exists guard means a message is visited exactly once (`bichon-archive.sh.j2:134`). The sidecar records the tag state that existed ~1h after arrival: empty, permanently.

So injecting the header — the obvious symmetric fix, and what `bichonctl`'s own mbox export does (`crates/cli/src/api/download.rs:80`) — would faithfully preserve an empty list forever. The bug is not the transport, it is that mutable metadata was assigned an append-only mechanism.

Rewriting one small file per account per run is the mechanism that fits. It needs no cursor and no change detection, because the expensive question ("which messages changed?") only arises when the payload is too large to re-emit wholesale. Tags are not.

Two observations then collapse the rest of the design:

- **A restore script is unavoidable.** No stock tool ingests `tags.json`. Once a script exists, folder can come from the sidecar, and the archive layout is free — so it should not change.
- **`bichonctl` was never a non-interactive option.** Its EML import `dialoguer`-prompts for the root directory (`crates/cli/src/eml/mod.rs:40`), which is precisely why ADR-0006 rejected `bichonctl` for archiving. Depending on it for restore would have reintroduced the coupling to upstream prompt wording that ADR-0006 refused.

**Tags are the least recoverable artifact in the system**, which is what justifies spending anything on them while they are unused. Bodies are in the Archive; folders are in the Archive and upstream; tags have exactly one copy, in the **Internal Store** — the store ADR-0006 deliberately does not back up, and the one an upstream `UIDVALIDITY` change purges. They are additionally the only artifact whose loss costs operator attention rather than mechanical re-derivation.

The hazard that surfaced this: on `UIDVALIDITY` mismatch Bichon calls `rebuild_mailbox_cache`, which deletes the folder's envelopes and their blobs before refetching (`crates/core/src/cache/imap/download/{flow.rs:469,rebuild.rs:210}`). For mail already expunged from the **Upstream Mailbox**, the refetch cannot restore it. That is a partial instance of the total-loss case ADR-0006 already addressed, so it needs the same remedy — but only if that remedy is faithful.

## Considered alternatives

- **Encode folder in the path (`<account>/<folder>/<id>.eml`) and restore with stock `bichonctl`.** Rejected. It requires dropping date partitioning entirely (see above), turning a large folder into a single directory of 100k+ files; it forces a full corpus migration whose window races the hourly timer, since changing the path defeats skip-if-exists and triggers a full re-download; and the "stock tooling" benefit is illusory because `bichonctl`'s importer is interactive and a script is needed for tags anyway.

- **Inject `X-Bichon-Metadata` into every `.eml`.** Rejected on two counts. It preserves an empty tag list (above), so it does not solve the problem. And backfilling it rewrites every byte of the corpus, costing one full re-upload and contradicting ADR-0006's priority 3 (restic-friendly dedup) — for nothing. It also stops the archived `.eml` being the byte-exact original, weakening priority 1.

- **Keep tags in the sidecar; refresh sidecars on a periodic full sweep.** Rejected: this is ADR-0006's already-rejected "pure skip-if-exists, no cursor" alternative returning under another name — paginating the entire envelope list to discover what changed, ~1000 API calls per sweep for a 100k-message corpus. The Tag Snapshot pages only the _tagged_ subset, so cost scales with a set the operator controls and is one call when that set is empty.

- **Backfill existing sidecars to drop the vestigial `tags` field.** Rejected: it buys tidiness at the cost of rewriting every sidecar in the corpus, and the field is inert — the restore script reads `folder` only. Documented as vestigial instead.

- **A `NoSync` vault account inside Bichon.** Bichon supports accounts of type `NoSync` (`crates/core/src/account/migration.rs:64`) which are never scheduled for sync (`trigger_start` is gated on `AccountType::IMAP`, `migration.rs:319`; `process_imap_download` asserts it, `download/mod.rs:45`) and whose imported mailboxes carry no `UIDVALIDITY` (`crates/core/src/import/mod.rs:92-105`). Continuously importing the Archive into such an account would make it structurally immune to a rebuild, and would additionally pin the shared blobs, since `cleanup_unused_content` deletes a blob only when no envelope anywhere still references its hash (`store/tantivy/envelope.rs:823`). Rejected: it is eager re-import where ADR-0006 chose lazy, it places the durable searchable copy inside the store we deliberately do not back up, it introduces a component that can go silently stale, and it needs its own dedup ledger because import assigns a fresh `Uuid::new_v4()` per envelope (`extractor.rs:160`) and is therefore not idempotent. Once the restore path is faithful, it buys nothing the Archive does not already provide.

- **Accept the loss and document it.** Rejected for tags specifically, on the silent-vs-loud principle of ADR-0007 §1. An operator who starts tagging gets no signal that the taxonomy is undurable; the loss surfaces only at restore, possibly years later, and is unrecoverable by construction. Accepted for the residual `Message-ID` gap below, where the exposure is bounded and rare.

## Consequences

**Positive:**

- Restore recovers bodies, folders, and tags, non-interactively, with one script. ADR-0006's claim becomes true as written.
- Nothing on disk moves. No migration window, no race with the archive timer, no restic re-upload — the change is additive.
- Adopting tags later requires no change and no memory of this analysis. Until then the Snapshot costs one API call per hourly run.
- Restore no longer depends on `bichonctl`'s interactive prompts, closing the upstream-coupling ADR-0006 objected to.
- Each fact has one home: bodies in the append-only tree, folder in the sidecar, tags in the rewritten Snapshot.

**Negative:**

- Messages arriving without a `Message-ID` header get a synthetic one (`extractor.rs:352` — timestamp, pid, random), which is not stable across re-import. Those messages lose their tags on restore. Rare, mostly automated senders, and it costs tags rather than content. Not mitigated.
- `tags.json` is rewritten hourly, so every snapshot holds a new copy. Negligible at realistic tag counts, but it is the first Archive artifact that does not dedup across snapshots.
- Sidecars written before this ADR carry an inert `tags` field. Harmless but untidy, and a reader who trusts it will conclude the corpus has no tags — which is true today and may not be later.
- Folder is still captured at first sight. A message moved between folders upstream after archiving keeps its original sidecar value. Accepted: moves are rare, and a message whose upstream folder changed within the 24h overlap window is re-archived under a second envelope id — pre-existing behaviour, unchanged by this ADR.
- A `UIDVALIDITY` purge still costs searchability until an operator notices and restores. Detection is out of scope here; Bichon logs the trigger at info level (`flow.rs:490`) and sets the folder status to `UID validity changed, rebuilding...`, so a journal alert is the cheap follow-up.

## References

- ADR-0006 — Bichon archive feeds the Backup Recipe. This ADR corrects its restore-fidelity claim; its Consequences describe restore as slower than rsync-and-go, but not as lossy.
- ADR-0007 — Auberge owns folder reconcile. §1's silent-vs-loud asymmetry is what decides the tag question: undurable tags fail silently and permanently. The reference-script-not-binary standing of `bichon-restore.sh` follows its Amendment's treatment of `bichon-expunge.sh`.
- CONTEXT.md — defines **Internal Store**, **Email Archive**, **Tag Snapshot**, **Upstream Mailbox**.
- Bichon import chain: `crates/cli/src/eml/mod.rs:34` → `crates/cli/src/api/sender.rs:33` → `POST /api/v1/import` → `crates/core/src/import/mod.rs:69` → `extract_envelope_core`.
- Bichon tag APIs: `GET /api/v1/all-tags` (`crates/server/src/rest/api/message.rs:327`), `POST /api/v1/update-tags` (`:344`), `EmailSearchFilter.tags` (`crates/core/src/message/search.rs:55`).
