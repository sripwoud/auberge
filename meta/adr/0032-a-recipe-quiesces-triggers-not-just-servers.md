# ADR-0032: A Backup Recipe quiesces triggers, not just servers

## Status

Accepted, 2026-08-23. **Extends ADR-0001** — a Recipe stays dumb data; this ADR gives two of its existing fields meaning they did not have: `systemd_services` may name any unit type, and its order is load-bearing.

## Decision

A Backup Recipe's `systemd_services` names **systemd units of any type**, not only `.service`s, and **the list order is the quiesce order**: whatever can start an App is listed ahead of the App itself.

For bichon that is `[bichon-archive.timer, bichon]`. A bare entry still means a `.service`, so every existing Recipe keeps its meaning unchanged.

## Why

`backup create`/`restore` stop exactly `recipe.systemd_services`. Bichon's Recipe named only the server, so `bichon-archive.timer` — hourly, `RandomizedDelaySec=10min`, `Persistent=true` — stayed active through the window. `bichon-archive.service` carries `Requires=bichon.service`, so a tick landing mid-rsync starts the archive, which pulls the deliberately-stopped server back up while its state is being copied (#619).

**Stopping a server whose trigger still ticks does not quiesce it.** The window's whole purpose is that nothing writes across it, and a Recipe that names only servers cannot express that when something other than an operator can start one.

The cost changed under ADR-0031. Pre-#617 the copied path was the append-only maildir, where every write is `mktemp`-then-`mv` and a torn run cost at most a refetch. Since the Internal Store joined the Recipe, the window covers live SQLite (`meta.db`, `mailbox.db`) and Tantivy segments: a restart mid-copy can snapshot a torn store, or — restore side — let the server write into a half-restored one. A narrow probability against a 1.6G multi-minute window, every nightly run.

Order rather than a second field (`stop_first:`, `triggers:`) because the executor already walks the list in order and the operator already reads it top-to-bottom. Naming the dependency twice invites the two spellings to disagree.

## Consequences

**Positive:**

- The Recipe can express quiescing for any App whose units something else can start — timers, sockets, paths — without a code change per App.
- `bichon-uidvalidity-watch.timer` stays out on a stated test rather than an oversight: it has no edge into the server and writes only `/var/lib/bichon-uidvalidity-watch`, outside every backed-up path.

**Negative / accepted:**

- **The preflight had to learn unit types.** `unit_file_name` appended `.service` unconditionally, so `bichon-archive.timer` resolved to `bichon-archive.timer.service`, matched nothing in `systemctl list-unit-files`, and made `validate_cross_host_restore` bail. ADR-0001's "recipes are data" holds for the Recipe; it did not hold for the guard reading it, which is the same shape as ADR-0027's markers — a check encoding an assumption the data outgrew. Scoped to `restore --from-host`, which is exactly the disaster-recovery path #615 exercised.
- **Restart walks the same order**, so the trigger starts before the server. Safe, and for the same reason the race exists: `Requires=`+`After=` make systemd start `bichon.service` first and wait for it, so a `Persistent=true` catch-up tick cannot run the archive against a down server. It leans on `bichon-archive.sh`'s `curl --retry`, because `Type=simple` reports active before bichon is listening.
- **The order is only as good as the unit edges it assumes.** `tests/bichon_backup_quiesce.rs` pins them — delete `Requires=bichon.service` and #619 returns silently, with every other test still green.

## References

- Issue #619 — the race, and why ADR-0031 is what made it matter.
- ADR-0001 — recipes are data; extended, not reversed.
- ADR-0031 — put the Internal Store in the window this ADR protects.
- ADR-0027 — the same failure shape: a guard encoding what reality used to be.
- ADR-0026 — the other place restore's contract is stated as data, not defaults.
