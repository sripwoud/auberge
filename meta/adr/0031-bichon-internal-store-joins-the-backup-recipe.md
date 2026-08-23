# ADR-0031: Bichon's internal store joins the Backup Recipe

## Status

Accepted, 2026-08-23. **Amends ADR-0006** — the Email Archive remains the durable, tool-agnostic copy of mail bodies and everything ADR-0006 built stands; this ADR reverses only its "internal store is not backed up" half.

## Decision

Bichon's Backup Recipe rsyncs `/opt/bichon/data` alongside `/var/lib/bichon-archive`. One line in `bichon.meta.yml` (`paths:` gains the store), no new code path — exactly the recipe-as-data property ADR-0001 promised.

## Why

ADR-0006 judged the Internal Store a derived, rebuildable cache and scored it against three operator priorities for _mail content_ — control, tool-agnostic format, restic dedup. That judgement was right for the cache and blind to a passenger. The store conflates two kinds of state:

- **Derived**: the Tantivy index, the content-addressed blob store, and `mailbox.db`'s envelope metadata — rebuildable from the Upstream Mailbox or replayable from the Email Archive.
- **Original**: `meta.db` — account definitions (IMAP endpoints, credentials, OAuth2 grants) and the API-token registry. Created interactively in the UI (Account Reconcile writes only `sync_folders`, ADR-0007), derivable from nothing, existing nowhere else.

The 2026-08-21 fleet cutover restore proved the cost (#615). The maildir landed on the new Host byte-perfect — 951M, sha-parity with the source — and the App was still dead in every way that matters: the fresh server had no accounts, so the vault's API token matched nothing in the registry, `bichon-archive.service` 401'd every hour with zero successful runs, the web UI rendered an empty archive, and even `examples/bichon-restore.sh` — ADR-0006's own restore path — was unrunnable for want of an authenticated account to import into. Every route back to a working install ran through manual UI work: re-create each account, redo OAuth2 consent, re-mint or re-register the token. No snapshot ever taken contained `meta.db`; the only copy in existence sat on a host scheduled for decommission.

ADR-0006 rejected the dual channel because the Archive "strictly dominated" the store on every stated priority. True for message bodies; false for the registry, which the Archive does not carry at all. A rejection premised on strict domination does not survive a counterexample.

## Considered alternatives

- **Back up only `meta.db`.** Rejected: it couples the Recipe — dumb data (ADR-0001) — to Bichon's internal file layout, which upstream may rename or split between releases. And a registry-only restore still buys hours of replay: 951M of bodies re-imported, then re-indexed, before the UI answers a search. The incident showed continuity is what this channel is for; the whole directory makes restore rsync-and-go.
- **Automate account re-provisioning** (role creates accounts, registers the token). Rejected: OAuth2 consent is interactive by construction and cannot be replayed unattended; credentials would transit a second secret channel wrapped by the non-rotatable encryption password; and it contradicts ADR-0007's boundary that reconcile is the only Bichon state auberge writes.
- **Keep ADR-0006 and accept the manual rebuild.** Rejected by the incident itself: the manual path was not performed under cutover pressure, and its cost compounds — every hour without accounts is an hour the archive timer fails, so the safety channel decays while the operator rebuilds the other one.

## Consequences

**Positive:**

- Host-loss restore becomes rsync-and-go: accounts, token registry, tags, and searchability return with the store. The Archive replay path remains for what it was built for — purge recovery (a store restored from backup would be re-purged by the same `UIDVALIDITY` sync) and Bichon-independent reading.
- The vault token's server-side half is finally durable; the token in `/etc/default/bichon-archive` and the registry entry it must match now travel in the same snapshot.

**Negative — ADR-0006's documented costs return, accepted with eyes open:**

- Restic dedup degrades: Tantivy segments rewrite on commit and encrypted rows re-chunk, so each snapshot carries churn proportional to the store (~1.6G today), not to newly-arrived mail. The Archive still dedups perfectly; the bloat is bounded to the store's paths.
- A restored store is useful only under the same non-rotatable `bichon_encryption_password`. Backing it up does not shrink that blast radius — the Email Archive remains the copy that survives losing the password, which is why this ADR amends ADR-0006 rather than replacing it.
- A staged backup from before this change lacks `/opt/bichon/data`, and restore rsyncs every declared path from the stage (`staged_paths`, ADR-0026) — so restoring a pre-#615 bichon stage fails fast at the missing path instead of silently restoring half an App. The remedy is the fresh snapshot #615 prescribes, not tolerance in the restore.

## References

- Issue #615 — the cutover restore that surfaced the gap; carries the live evidence table.
- ADR-0006 — amended by this ADR: its Archive design and priorities stand for mail content; its store-is-rebuildable premise is narrowed to the store's derived half.
- ADR-0001 — recipes are data; this fix is a one-line `paths:` edit.
- ADR-0007 — account creation is UI-driven, which is what makes `meta.db` original state.
- ADR-0026 — restore restores what the staged backup holds; why old stages now fail fast.
