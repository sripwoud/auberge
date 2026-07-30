# ADR-0013: Message identity in the Email Archive is the Message-ID, read from the body

## Status

Accepted, 2026-07-30. **Amends ADR-0012** — it does not replace it. The layout, the Tag Snapshot, and the restore script all stand; this ADR adds one field to the sidecar and reverses one of ADR-0012's rejections.

## Decision

The per-message sidecar records the message's identity alongside its folder:

```
/var/lib/bichon-archive/<account>/YYYY/MM/<envelope-id>.eml         unchanged
/var/lib/bichon-archive/<account>/YYYY/MM/<envelope-id>.meta.json   {folder, message_id}
/var/lib/bichon-archive/<account>/tags.json                         unchanged
```

- `message_id` is the RFC 5322 Message-ID of the message, **read from the archived `.eml`**, not from Bichon's `search-messages` envelope.
- Canonical form: unfolded, whitespace-trimmed, stripped of one pair of surrounding angle brackets.
- A body carrying no Message-ID header is keyed `sha256:<hex>` over the `.eml`.
- Existing sidecars are **backfilled** by a self-healing pass inside `bichon-archive.sh`, which rewrites only sidecars lacking the field. The rewrite drops the vestigial `tags` field as a side effect.
- Gate 3 of `examples/bichon-expunge.sh` counts **distinct `message_id`** instead of `.eml` files. A sidecar without the field aborts the gate, naming the file and the command that repairs it. There is no fall back to counting files.
- `<envelope-id>` remains the filename. It is a storage detail, not an identity — it does not survive re-import.

## Why

ADR-0012 separated what is immutable (bodies, folder-at-first-sight) from what is mutable (tags). It did not ask which of the two the archive's **dedup key** came from. It came from the mutable side: the only key was the `<envelope-id>.eml` filename, and Bichon mints a fresh `Uuid`/id per envelope on every import (`extractor.rs:160`, cited in ADR-0012's own rejection of the `NoSync` vault). One upstream message can therefore occupy several `.eml` files.

That is a safety defect rather than an untidiness, because of who consumes the count. Gate 3 of `examples/bichon-expunge.sh` is the last check before an operator permanently deletes mail from an **Upstream Mailbox**, and it asserted coverage as:

```bash
((eml_count >= IMAP_COUNT))
```

`eml_count` was a `wc -l` of sidecars. Every duplicate copy raised it without covering an additional upstream message, so the gate passed on less evidence than it appeared to demand. A gate that is _subtly_ wrong is not loud — it prints a green check against a corpus it has miscounted, which inverts the silent-vs-loud test that decided ADR-0007 §1.

### The measurement that scoped this, and the one that corrected it

Issue #389 reported 103 surplus copies across 5 of 7 accounts: 4,954 `.eml` files against 4,851 distinct `(account, Message-ID)` pairs. Re-measuring on the live archive reproduced 4,851 exactly — and then showed the figure to be an artifact of how Message-ID was read, not a property of the corpus:

| Extraction of `Message-ID` from the `.eml` | Distinct | Surplus |
| ------------------------------------------ | -------- | ------- |
| line-anchored `^Message-ID:` (as in #389)  | 4,851    | 106     |
| folding-aware, angle brackets stripped     | 4,956    | **1**   |
| the above, `sha256` for a header-less body | 4,959    | **0**   |

RFC 5322 permits a header value on a continuation line, and Exchange routinely folds Message-ID:

```
Message-ID:
 <AM6PR02MB5160…@AM6PR02MB5160.eurprd02.prod.outlook.com>
```

108 of 4,959 bodies are folded that way. A line-anchored match reads each as empty, and all of them collapse onto one empty key per account — manufacturing ~106 phantom duplicates. The single real collision was two `Email Quota Usage` notifications that carry no Message-ID header at all, colliding on the same empty key.

**So the corpus holds no duplicate bodies today, and the fix is preventive rather than remedial.** The mechanism is nevertheless real and unguarded: nothing in the archive prevents it, and the archive's cursor is the only reason it has not fired. The cursor advances to the newest `date` seen, so a message re-imported under a fresh envelope id keeps its old `Date` header and is never re-examined — no duplicate, and no second download. Any full pass (cursor reset, migration, folder reorganisation) removes that accident and re-downloads the entire corpus under fresh envelope ids, doubling the file count while the message count is unchanged. Landing this before such a pass is the whole point; #389's "Blocks" note is correct even though its headline number was not.

The correction also raises the standard for the fix. Two of the three extractions in the table above are wrong, and the wrong one is the obvious one — so the extractor is the part that needs tests, not the count.

### Why the identity is read from the body and not from Bichon's envelope

#389 scoped the field as "`message_id` is already in the `search-messages` envelope the script holds — no extra API call," and asked for confirmation against the live response before relying on it. Confirmation found the field present on all 5,448 envelopes and never null, and also found that it is **not** the header value:

| Envelope `message_id`                             | Count | Form                                     |
| ------------------------------------------------- | ----- | ---------------------------------------- |
| `AM6PR02MB5160…@…outlook.com`                     | 5,391 | header value, angle brackets stripped    |
| `<000848cd44fec49c.1778017746410.1554362@bichon>` | 57    | Bichon's synthetic id, brackets **kept** |

Of 4,956 archived messages joinable to a live envelope, exactly **1** matched its own `.eml` header byte-for-byte; all the rest differ by brackets, by folding whitespace, or both. So a design where new sidecars take the envelope field and backfilled sidecars take the header has two producers of one key that disagree on its form — which is the same class of defect as the one being fixed, reintroduced in the fix. Normalising both to one canonical form is possible, but it leaves two normalisation paths to keep in step across an upstream that never documented the one it applies.

Reading the identity from the `.eml` removes the second producer. One function serves the writer and the backfill, so the tests cover the production path, and the value cannot drift if Bichon changes how it stores the field. It costs one `awk` pass over a file already on local disk, and still no API call.

It is also the correct level. ADR-0012's thesis is that each fact belongs with the mutability it has. The archive's identity key is a property of the message, and the archive's copy of the message is the `.eml` — the artifact that is byte-exact and never revisited. Bichon's envelope is an index entry that is rebuilt on `UIDVALIDITY` change and re-minted on import. Taking identity from the index was the original defect; taking it from the index in a normalised form would be the same defect with a smaller blast radius.

### Why a header-less body is keyed by its own hash

Bichon synthesises a Message-ID for such messages (`extractor.rs:352` — timestamp, pid, random) and ADR-0012's Negative 1 records that it does not survive re-import. Three of 4,959 bodies are affected. Three options:

- **Empty key.** Rejected: it is what produces the one real collision measured above. Two distinct messages sharing a key means the gate credits one where there are two.
- **Bichon's synthetic id.** Rejected: unstable across re-import — so it would leave the duplication mechanism live for exactly the messages that have no other identity — and unavailable to the backfill, which reads no API. One archived `.eml` already has no live envelope at all.
- **`sha256` of the `.eml`.** Accepted. Stable across re-import as long as the download is byte-exact, which ADR-0012 already guarantees; available identically to both producers; distinct for distinct bodies. It closes ADR-0012's residual for the gate's purpose, without claiming to close it for tags — `tags.json` still keys on Bichon's id, and those three messages still lose their tags on restore.

### Why the backfill runs inside the hourly archive

ADR-0012:62 rejected backfilling existing sidecars. This ADR reverses that rejection on the ground the rejection itself named:

> **Backfill existing sidecars to drop the vestigial `tags` field.** Rejected: it buys tidiness at the cost of rewriting every sidecar in the corpus, and the field is inert.

Both clauses are load-bearing and both change here. What backfill buys is no longer tidiness but the correctness of the one gate standing between an operator and unrecoverable deletion — and the field is not inert, it is the value the gate counts. A partly-keyed corpus is worse than either extreme, because the gate would have to either fail on every run or guess.

The cost is also not what ADR-0012 was guarding. Its §58 refused injecting `X-Bichon-Metadata` because backfilling it "rewrites every byte of the corpus, costing one full re-upload" and contradicting ADR-0006's priority 3. A sidecar is ~38 bytes: rewriting 4,959 of them is a restic delta on the order of 300 KB against a corpus of bodies that does not move at all. The `.eml` files — the bytes priority 3 exists to protect — are not touched.

Three places could host the pass. **A one-shot ansible task** couples convergence to a deploy, and a sidecar arriving unkeyed later (a rolled-back binary, a restore from an old snapshot) would not be repaired. **A standalone script** is a fourth artifact under `examples/` that an operator must know to run, at the exact moment they are being told the gate refuses to proceed. **A self-healing pass in `bichon-archive.sh`** converges on the next hourly tick with no operator action, is discovered by one `grep -L` per account, and needs nothing new installed. That is the choice.

It does revisit entries ADR-0012 called write-once, so this ADR sanctions the revisit explicitly and narrowly:

- The pass reads **only** the sidecar's existing `folder` and the sibling `.eml`. It makes no API call, so first-sight `folder` (ADR-0012:83) cannot be overwritten by a later value — the property that made write-once the right rule is preserved by construction, not by discipline.
- It recomputes a **derived index key**, not an observation. The input is immutable, so the pass is idempotent: a second run finds nothing to do.
- It rewrites only sidecars missing the field. A sidecar that already carries one is never re-derived, so a key written under one extraction is never silently replaced by another.

Write-once remains the rule for observations about a message. It was never a rule about the file's byte layout, and this ADR makes that distinction explicit rather than leaving it to be re-derived.

## Considered alternatives

- **Leave the gate counting files and document the inflation.** Rejected on ADR-0007 §1. The gate's failure is silent in the dangerous direction: it over-reports coverage, and the operator learns nothing until mail that existed in one place is gone. Documentation does not reach the person running the script at the moment they need it.

- **Count distinct `.eml` content hashes instead of Message-IDs.** Tempting — it needs no sidecar change at all and dedups byte-identical duplicates. Rejected: it hashes the whole corpus on every gate run rather than reading ~38-byte sidecars, and it answers a slightly different question. Two copies of one message that differ by so much as a `Received:` line added at re-download would count as two. Message-ID is the identity the **Tag Snapshot** already uses (ADR-0012:20) and the identity RFC 5322 assigns; using two different identities in one archive is how the next inconsistency gets built.

- **Normalise the envelope field and use it for new writes, headers only for the backfill.** Rejected above: two producers of one key, disagreeing on form, with the normalisation of one of them undocumented upstream.

- **Have the gate tolerate a missing `message_id` by falling back to the file count for those sidecars.** Rejected. A partial file count is the inflation this ADR removes, reintroduced as a special case, and it would make the backfill's completion unobservable — the gate would pass identically whether the corpus was keyed or not.

- **Fail the archive run when a message has no Message-ID header.** Rejected. Non-conformant senders are not an archive defect, and the run would never converge: three messages would block the gate permanently, which is a denial of service on every future expunge rather than a signal about anything actionable.

- **Key sidecars on `(account, Message-ID)` rather than Message-ID alone.** Rejected as redundant. The sidecar already lives under `<account>/`, and every consumer scopes to one account directory. Storing the account inside the file would put the same fact in two places, which is what ADR-0012 spent its Consequences avoiding.

## Consequences

**Positive:**

- Gate 3 counts messages. Its verdict no longer moves when the archive re-downloads a message it already holds.
- A partly-keyed corpus is unrepresentable in practice: the gate refuses it loudly and the repair is one `systemctl start`, printed in the refusal.
- Identity is now stable across the operation that breaks everything else. A `UIDVALIDITY` purge and re-import changes envelope ids, filenames, and Bichon's synthetic ids; it does not change a Message-ID or a body hash.
- The extraction has 24 assertions behind it, including the folding cases that produced #389's phantom count. The reader that was wrong is now the reader that cannot regress silently.
- ADR-0012's Negative 3 is discharged. The backfill drops the vestigial `tags` field, and it does so as a side effect of a rewrite already justified on other grounds — which is the only basis on which ADR-0012 would have accepted it.
- Sidecars now agree with `tags.json` keys for the 5,391 messages whose envelope field is bracket-stripped, so a future cross-check between the two artifacts is cheap. Nothing depends on this today.

**Negative:**

- The archive reads every sidecar of every account on every hourly run to find the ones needing repair. One `grep -L` over ~5,000 files of ~38 bytes, mostly page-cached; it grows linearly with the corpus and will want revisiting long before it matters.
- The 57 synthetic-id messages have a sidecar key (bracket-stripped, or a body hash) that differs from their `tags.json` key (brackets kept). Nothing joins the two artifacts, so this is latent, but a future cross-check must normalise.
- A body whose `.eml` is not byte-identical across re-download gets a second key if it has no Message-ID header. ADR-0012:18 guarantees byte-exactness, so this is a bounded consequence of that guarantee failing rather than an independent risk.
- Message identity is now read by two implementations — `canonical_message_id` in the role template, and nothing else. That is the point, but it means the reference scripts under `examples/` must not grow a second reader; they consume `message_id` from the sidecar.
- The corpus holds one 0-byte `.eml`, which the archive wrote because `curl --fail` treats an empty 200 as success. This ADR keys it by the hash of nothing rather than failing on it. Out of scope, tracked separately.

## References

- **ADR-0012** — this ADR amends it. §62's rejection of sidecar backfill is reversed on the grounds §62 itself named; §58's cost objection is distinguished (sidecars, not bodies); §83's first-sight `folder` is preserved by the backfill reading no API; §80's synthetic-Message-ID residual is closed for the coverage gate and left open for tags.
- ADR-0007 — §1's silent-vs-loud asymmetry is what makes this a defect rather than an untidiness, and its Amendment establishes `bichon-expunge.sh`'s standing as a reference script that may execute the expunge.
- ADR-0006 — priority 3 (restic-friendly dedup) is what bounds the acceptable backfill cost; the cursor plus 24h overlap it established is why the duplicate mechanism has not yet fired.
- CONTEXT.md — **Email Archive** (message identity), **Tag Snapshot**, **Internal Store**, **Upstream Mailbox**.
- RFC 5322 §2.2.3 (folded header fields), §3.6.4 (`msg-id`).
- Issue #389 — reported the defect. Its 103-surplus measurement is corrected above; its analysis of the mechanism and its ordering constraint stand.
