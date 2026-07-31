# ADR-0015: The Email Archive publishes a downloaded body only if it is a message

## Status

Accepted, 2026-07-30. Closes the last Negative of **ADR-0013**, which recorded the defect and deferred it:

> The corpus holds one 0-byte `.eml`, which the archive wrote because `curl --fail` treats an empty 200 as success. This ADR keys it by the hash of nothing rather than failing on it. Out of scope, tracked separately.

## Decision

`bichon-archive.sh` publishes a downloaded payload as `<envelope-id>.eml` only if it carries a **header block**: at least one header field line, then the empty line RFC 5322 §2.1 puts between the headers and the body.

- A payload that fails is deleted, logged with its byte count, and counted as an account failure. The cursor does not advance, so the next run retries.
- The skip guard asks the same question of a body already on disk. Existence is not the test — a 0-byte file satisfies `[ -f ]`.
- A body already in the archive that fails the check is re-downloaded by the envelope id its filename carries, and its sidecar re-keyed from the refetched body. Self-healing pass in the hourly run, placed and justified as ADR-0013's sidecar backfill was.
- A body that cannot be refetched fails the unit on every run until an operator deletes the body and its sidecar.

## Why

`/var/lib/bichon-archive/x@riou.one/2026/03/3809033272456269.eml`:

|                                                  | Value                  |
| ------------------------------------------------ | ---------------------- |
| archived file                                    | **0 bytes**, mode 640  |
| envelope `size` in the Internal Store            | 40,609                 |
| `GET /download-message/<acc>/<id>` on 2026-07-30 | HTTP 200, 40,609 bytes |

The body is intact upstream and intact in the **Internal Store**. The **Email Archive** — the artifact ADR-0006 designates as the durable copy — held nothing, and by construction would never have asked again:

- `--fail` is about status codes, not payload length.
- `--retry-all-errors` retried nothing, because nothing reported an error.
- the skip guard was `[ -f "$eml_path" ] && [ -f "$meta_path" ]`, which a 0-byte file satisfies.
- the run logged `processed`, so `total_failures` stayed 0 and the unit went green — gate 2 of `examples/bichon-expunge.sh` sees a healthy archive.

Upstream, `download_message` streams `BLOB_MANAGER.get_email(content_hash)` (`storage.rs:229`). A **missing** blob is a 404; a **zero-length** blob is a 200 carrying no bytes. Nothing on either side of that call distinguishes it from a message.

It is a safety defect for the same reason ADR-0013's was. Gate 3 counts distinct `message_id` from the sidecars, and this body's sidecar is keyed `sha256:<hash of empty>` — so the gate credits one covered message against an `.eml` holding nothing, immediately before an operator permanently deletes mail from an **Upstream Mailbox**. Coverage inflation in the dangerous direction, which is the asymmetry ADR-0007 §1 decides on.

### The check was measured before it was chosen

Four candidates, run over all 4,960 archived bodies:

| Candidate                            | Rejects | Of those, real mail |
| ------------------------------------ | ------- | ------------------- |
| non-empty                            | 1       | 0                   |
| **header block** (accepted)          | **1**   | **0**               |
| first line is a header field         | 4       | **3**               |
| `size` equality against the envelope | not run | —                   |

The obvious strengthening — require the first line to look like a header field — is wrong, and wrong in the direction that never converges: three live bodies open with an mbox `From_` line (RFC 4155), which has no colon. That rule would refuse all three, refetch all three every hour, and fail the unit forever over mail the archive already holds correctly.

The header block costs nothing over the non-empty check and catches strictly more: a payload cut off inside the headers fails it. Reading stops at the separator, so it is one page of a body rather than all of it.

`size` was rejected on the source rather than on a measurement, and the distinction is worth recording. Bichon sets it as `fetch.size.unwrap_or(body.len() as u32)` (`extractor.rs:54`) — the IMAP server's `RFC822.SIZE`, a number the upstream server declares — while the bytes the API serves are rebuilt by re-attaching detached attachments (`extractor.rs:465`). Two producers, no documented reason to agree. One matching sample (40,609, above) does not establish an invariant, and an equality check that is systematically wrong for some class of message would refuse that whole class forever. Measuring it corpus-wide needs the API token on the Host, which was not worth a production credential for a check already rejected on construction.

### Why this failure is allowed not to converge

ADR-0013 rejected failing the run on a message with no `Message-ID` header, because three such messages would block the gate permanently — "a denial of service on every future expunge rather than a signal about anything actionable". This ADR fails the run on an empty body, and the two are not the same case:

- a missing `Message-ID` header is a property of the **sender**, and the archive holds the message regardless.
- an empty body is the archive **not holding the message**. There is nothing to report except that.

The blast radius is also smaller than "the cursor is frozen" suggests. A frozen cursor does not stop new mail being archived: the search runs from the stale cursor on every tick and still returns everything newer. It costs a repeated scan and a red unit, which is the intended signal.

### Why the repair runs inside the hourly archive

The download-time refusal cannot reach the body already in the corpus. That message is dated 2026-03; the account cursor is months past it and the overlap is 24h, so `search-messages` never offers its envelope again and the skip guard never runs on it. Repair has to start from the archive rather than from the search — which it can, because the filename **is** the envelope id.

The three candidate homes are the ones ADR-0013 weighed, and they resolve the same way: a one-shot ansible task couples convergence to a deploy and repairs nothing that arrives later from a rolled-back binary or an old snapshot restore; a standalone script under `examples/` is a fifth artifact an operator must know to run; a pass in `bichon-archive.sh` converges on the next tick with no operator action.

It revisits an entry ADR-0012 called write-once, and the sanction is narrower than ADR-0013's. Re-keying a sidecar was defended as recomputing a derived index key. Replacing a body is not even that: **a payload that was never a message is a write that did not happen**, so there is no observation to revise. The first-sight `folder` is preserved by construction — `rekey_sidecar` reads it from the sidecar and never from an API.

## Considered alternatives

- **Reset the account cursor once, by hand, and let the skip guard do the rest.** It works: a full pass re-lists every envelope, skips the 4,959 valid bodies for the cost of one `awk` each, and re-downloads the one that is not a message. Rejected as the only repair, for the reason ADR-0013 rejected a one-shot: it has no memory. The next body written by a rolled-back binary is stranded exactly as this one was, and nobody is told.

- **Delete the empty `.eml` and let the main loop re-archive it.** Rejected: the loop only sees envelopes the search returns, and this one has aged out — so the deletion is not a repair, it is the archive quietly losing an entry. It also takes the sidecar's first-sight `folder` with it, which nothing can recover.

- **Have gate 3 ignore sidecars whose body is implausible.** Rejected: it moves the check to the consumer, and ADR-0013 closed with the rule that the reference scripts under `examples/` must not grow a second reader of the archive's contents. The archive would also stay wrong while every consumer compensated.

- **Compare the written bytes to the envelope's `size`.** Rejected above.

## Consequences

**Positive:**

- Gate 3 no longer credits coverage to a body the archive does not hold.
- The corpus converges on the first tick after deploy, with no operator action: one body refetched, one sidecar re-keyed from `sha256:` of nothing to its real `Message-ID`.
- Both halves of the defect are now unrepresentable rather than merely absent: a payload that is not a message cannot be published, and one that is already published cannot be skipped.
- 30 new assertions in `tests/bichon-archive.test.sh` (31 → 61), including the mbox `From_` case that ruled out the stricter check. The one function reaching Bichon is stubbed, so the suite still contacts nothing.

**Negative:**

- A second full-corpus scan per run — `find` plus one `awk` per body — alongside ADR-0013's `grep -rL` over the sidecars. The `awk` stops at the separator, so it reads a page per body rather than a whole one, but the two passes now grow together and should be revisited together.
- A body whose envelope Bichon no longer holds fails the unit on every run, and nothing repairs it automatically. Clearing it is an operator deleting both the body and its sidecar — deleting only the body leaves the backfill failing with `reason=no_body` instead.
- The check accepts any payload carrying a header block, so a body truncated _after_ the separator is still published. curl catches a short transfer that reports a length or breaks a chunked stream; a genuinely short, well-formed 200 is undetectable without a second source of truth for the length, which is the option rejected above.
- Every archived body is now read on every run, so an unreadable body is counted as "not a message" and refetched. The archive owns its own files, so this is reachable only by an operator changing modes under it.

## References

- **ADR-0013** — this ADR closes its final Negative. Its placement argument for a self-healing pass is reused; its rejection of a non-converging failure is distinguished, not overturned.
- ADR-0012 — §83's first-sight `folder` is preserved by `rekey_sidecar`, which reads it from the sidecar and never from an API; ADR-0012's write-once rule is narrowed further here.
- ADR-0007 §1 — the silent-vs-loud asymmetry that makes an over-credited gate a defect.
- ADR-0006 — the Archive is the durable copy; an empty body is a hole in the artifact the backup exists to protect.
- CONTEXT.md — **Email Archive**, **Internal Store**, **Upstream Mailbox**.
- RFC 5322 §2.1 (the empty line between headers and body).
- Bichon `crates/core/src/store/storage.rs:229`, `crates/core/src/envelope/extractor.rs:54`.
- Issue #393 — reported the defect, with the proof reproduced above.
