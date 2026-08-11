# ADR-0019: The Archive's download skip is a Message-ID membership test

## Status

Accepted, 2026-08-11. **Completes ADR-0013** — that ADR made the Message-ID the archive's identity and fixed the consumer that counted wrong (the coverage gate). It left the producer asking a different question. This one aligns them.

## Decision

The hourly run decides whether to write a body by asking whether the archive already holds that **message**, not whether it holds that **filename**.

- Each account's set of archived `message_id`s is read from its sidecars at the top of its pass, after the two repair passes have finished rewriting them, and is added to as the run publishes.
- A body whose `<envelope-id>.eml` is present and is a message, with a sidecar beside it, is still skipped on the filename. That is the cheap answer and it is correct for every message archived under the envelope id regime in force.
- Otherwise the body is downloaded to `<envelope-id>.eml.incoming`, keyed with `canonical_message_id`, and moved into place **only** if the account's set does not already hold that key. A duplicate is discarded unpublished.
- A body already published whose sidecar is missing is keyed where it lies and is not a dedup candidate.
- The set failing to build fails the account's pass without downloading anything. The cursor is not written.
- The run logs `skipped` (filename match) and `deduped` (identity match) separately.

## Why

ADR-0013:22 recorded that `<envelope-id>` "is a storage detail, not an identity — it does not survive re-import", and ADR-0013:55 predicted the consequence precisely:

> Any full pass (cursor reset, migration, folder reorganisation) removes that accident and re-downloads the entire corpus under fresh envelope ids, doubling the file count while the message count is unchanged.

The download skip was nevertheless left as `[ -f "${id}.eml" ]`. Upgrading Bichon to 2.0.1 regenerated every envelope id as a UUID, and the guard reported all 9,394 archived bodies as absent. One tick wrote **825 duplicate files**; `skipped` for one account fell from 714 to 0 across the migration, which is the whole defect in one number.

Two things about how it fired are worth recording, because the issue's first analysis got both wrong:

- **It is not gated behind a full pass.** `bichon_archive_overlap_seconds: 86400` means every ordinary tick re-lists a 24h window, not only what arrived since the cursor. The mechanism ADR-0013 filed under "migration" was live on the hourly timer, compounding ~825 files/hour.
- **The corpus already held the fossils.** 53 sidecars across two accounts share a `message_id` with another sidecar in the same account and folder: 51 copies of one 187-byte `From`+`Subject` notification and 4 of another, all `sha256:`-keyed, all dated epoch 0, all numerically named. One copy per historical full pass — the predicted duplication, already in the corpus, invisible because the gate counts distinct ids.

### Why the identity is read after the download, not before

ADR-0013:68 measured Bichon's envelope `message_id` field against the archived bodies: present on all 5,448 envelopes and never null, but of 4,956 joinable messages exactly **one** matched its own `.eml` header byte-for-byte — the rest differ by brackets, by folding whitespace, or both. It rejected that field as a _second producer_ of the stored key, on the ground that normalising it leaves "two normalisation paths to keep in step across an upstream that never documented the one it applies."

A membership test built on the envelope field would re-admit exactly that, for a saving measured in one day of downloads. So identity stays readable only from the body, and the body has to arrive before it can be recognised.

That is what makes the download stage rather than publish. The staging name deliberately does not match `*.eml`: `repair_broken_bodies` sweeps that glob, and a body still under a dedup decision is not a body the repair pass may see.

### Why the filename check stays in front of it

It answers without bytes, and it answers for every message archived since the last id re-mint — in steady state, all of them. Removing it would download the whole 24h window every hour forever to learn what a `[ -f ]` already knew. Keeping it bounds the identity test to the mail whose id regime has actually moved.

### What a per-run scan costs

Measured on the live corpus, warm: **0.112s** for one `jq` over all 9,396 sidecars across 7 accounts, batched through `xargs` the way `examples/bichon-expunge.sh` batches its own walk (#406) — three `jq` processes for the largest account, not 5,446. `backfill_message_ids` already runs `grep -rL` over the same files for 0.06–0.11s. Against an hourly timer, an index would be a second source of truth bought with nothing.

ADR-0013:129 flagged that per-run walk as growing linearly and wanting revisiting "long before it matters". This adds a second pass of the same shape and does not change that assessment.

## Considered alternatives

- **Guard the `rescan` path instead.** Rejected: nothing was rescanned. The plain hourly tick caused this, through the 24h overlap window. A guard on `rescan` would have left the live defect untouched.

- **Rename the corpus to the new envelope ids.** Rejected. It is a large irreversible rewrite of every filename in a 914M corpus to avoid a problem the membership test solves anyway, and it would have to be redone on the next re-mint. Mixed numeric/UUID filenames are harmless once nothing reads a filename as identity — which is the point of ADR-0013 and now true of the producer as well.

- **Key the set on `(folder, message_id)`.** Tempting, because gate 3 counts coverage per folder (`sidecar_rows_for_folder`) and a message moved between folders upstream would, under Message-ID alone, be recognised and not re-archived under its new folder. Rejected: ADR-0012:83 records folder **at first sight** and never revises it, so a move is not a second message and must not become a second `.eml`. Folder-keying would turn the archive into a record of folder history, which no decision sanctions. Measured support: no duplicated `message_id` in the live corpus spans two folders — all 53 are same-folder.

- **Use Bichon's envelope `message_id` as a pre-download filter, with the body as the authority.** Rejected above on ADR-0013:68. It buys one overlap window of downloads and costs a second normalisation path against an undocumented upstream form.

- **Delete the redundant body instead of discarding the download.** Rejected. The archive does not delete; ADR-0012 makes bodies write-once, and choosing which of two copies of a message to destroy is an operator's call, not an hourly timer's. The 53 fossils stay.

## Consequences

**Positive:**

- The duplication mechanism ADR-0013 predicted and left live is closed at the producer. A re-mint of every envelope id — upgrade, `UIDVALIDITY` purge, restore — now costs downloads instead of files.
- A full pass is idempotent for real, which is what CONTEXT.md's **Archive Cursor** repair (reset to 0 and re-run) has always claimed and could not deliver under a changed id regime.
- `skipped` versus `deduped` makes the id regime observable. A sustained non-zero `deduped` says Bichon's ids moved; that was previously visible only as a rising file count.
- The identity read is still one implementation (ADR-0013:132). `write_meta_sidecar` now takes the id its caller already had to read, rather than re-deriving it.

**Negative:**

- A re-listed message whose filename does not match costs a download that is thrown away. Measured on the first run after this shipped: **773 discarded downloads** across 7 accounts (612 on the worst), ~75MB over localhost to Bichon's own API, against 1 message genuinely archived. Bounded by the overlap window — it decays to zero within 24h as the cursor advances past the old-regime mail, and the second run already showed it working: the message published by the first was matched by filename, not refetched.
- A full pass with the cursor reset to 0 now downloads the entire corpus (914M) to recognise it. Cheap in the sense that matters — it writes nothing it already holds — and expensive in a sense that was previously free.
- Two byte-identical bodies with no `Message-ID` are one message to this guard, because `sha256` is what ADR-0013 assigns them as identity. A repeat notification identical to one already archived is not archived again. This is ADR-0013's decision applied to the producer, not a new one; the coverage gate already counted them as one.
- A crash between staging a body and publishing it leaves an `.incoming` file. Invisible to every sweep (it matches neither `*.eml` nor `*.meta.json`), overwritten by the next attempt on the same envelope, and backed up until then.

## References

- **ADR-0013** — this ADR completes it. §22 (filename is storage), §55 (predicted this duplication), §68 (why identity is not read from the envelope), §129 (per-run sidecar walk), §132 (one identity reader).
- ADR-0012 — §18 byte-exact bodies, §83 folder at first sight, and write-once for bodies, which is why a redundant download is discarded rather than a redundant body deleted.
- ADR-0015 — the download refusal this skip is layered on: a payload is published only if it is a message. Its filename-derived refetch in `repair_broken_bodies` is untouched, and is why the staging name must stay outside `*.eml`.
- ADR-0006 — the 24h overlap window, which is what made the defect hourly rather than occasional.
- CONTEXT.md — **Email Archive** (message identity, download skip), **Archive Cursor**.
- Issue #455 — reported it. The issue body's severity call ("the hourly tick is safe") is corrected in its comments and above.
