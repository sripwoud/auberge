# ADR-0014: The UIDVALIDITY rebuild alert is a latched failing unit

## Status

Accepted, 2026-07-30. Executes the follow-up ADR-0012's Consequences named and left unbuilt.

## Decision

An hourly timer on the Bichon Host reads `bichon.service`'s journal from a saved cursor, appends every line matching `detected with changed uid_validity` to a **Rebuild Latch** file, and exits non-zero while that file is non-empty. The failed unit is the alert; the file is the state.

```
/var/lib/bichon-uidvalidity-watch/cursor         journalctl --cursor-file
/var/lib/bichon-uidvalidity-watch/rebuilds.log   the latch: one line per detected rebuild
```

- `ansible/roles/bichon/files/bichon-uidvalidity-watch.sh`, deployed to `/usr/local/bin`, driven by `bichon-uidvalidity-watch.timer`.
- Exit codes match `auberge backup verify` (**Backup Verdict**): `0` no rebuild recorded, `1` a rebuild is recorded and unacknowledged, `2` the journal could not be read.
- Acknowledgement is `rm rebuilds.log`, then `systemctl start bichon-uidvalidity-watch.service` to clear the failed state immediately rather than waiting a tick.
- Runs as `bichon` with `SupplementaryGroups=systemd-journal`, not as root.
- No new Key Registry entries, no network egress, no delivery credential.
- Out of scope, unchanged from ADR-0012: preventing the rebuild, and automating the restore.

## Why

ADR-0012 made the restore path faithful and closed with the one residual gap it declined to fix: "a `UIDVALIDITY` purge still costs searchability until an operator notices and restores. Detection is out of scope here … so a journal alert is the cheap follow-up." Bichon logs the trigger at info level and carries on. Nothing else noticed.

The signal is worth building on because it is stable and it precedes the damage:

| Property         | Evidence                                                                                                                                                                                                                             |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Emitted pre-loss | logged before `rebuild_mailbox_cache` is called, so it names the folder that is about to be purged                                                                                                                                   |
| Version-stable   | byte-identical in the deployed 0.3.7 (`src/modules/cache/imap/sync/flow.rs:283`) and at upstream HEAD (`crates/core/src/cache/imap/download/flow.rs:491`) — the workspace re-crate refactor moved the file and left the string alone |
| Reachable        | `bichon_log_level` defaults to `info`, so the line is in the journal as deployed                                                                                                                                                     |

**The alert has to be latched, not edge-triggered.** This is the decision; everything else follows from it. systemd clears a unit's failed state on the next successful start, so a rebuild announced by exit code alone is erased by the tick that follows it — at hourly cadence the alert would survive under an hour, against a hazard whose failure mode is measured in weeks. Persisting the finding to a file and re-failing on it turns a momentary event into a condition that stays until a human clears it. The operator's acknowledgement is a file deletion, which is also the record that they saw it.

**Why a failing unit rather than a push.** The Host runs Cockpit, so `systemctl --failed` is already a surface an operator sees without new infrastructure. Telegram was the alternative — the operator already provisions bot tokens for `hermes` and `tgtg`, so it would have worked — and was rejected for scope: it costs two Key Registry entries and a delivery credential inside the bichon role, and `meta/roadmap.md` still carries "Alerting (via email or webhook)" as an unbuilt cross-cutting item. A bichon-specific sender would be the thing that design has to unpick later. The latch is the smaller commitment and composes with a push if one is ever built: any notifier can watch one file or one unit state.

**Why not `journalctl --grep`.** It would push the filter into journalctl and remove the staging file, and it destroys the distinction the exit codes exist for: `--grep` exits `1` when nothing matched, which is also what an unreadable or misnamed unit returns. A renamed `bichon.service` would then read as "no rebuild" forever — the silent failure ADR-0007 §1 rejects, in the one unit whose entire job is to be loud. `grep -F` downstream keeps journalctl's status unambiguous, at the cost of staging the read through a file. Verified: `--cursor-file` does advance to the last entry _examined_ even when `--grep` shows nothing, so the cursor is not what rules it out.

**The first run reads the whole retained journal**, because no cursor exists yet. Deliberate: a rebuild that already happened is precisely what the operator was never told about, and seeding a silent baseline would bury it. This is also why the read is staged through a file rather than a shell variable — a VPS journal holding months of `bichon.service` is hundreds of thousands of lines.

## Considered alternatives

- **Telegram (or any push) from the bichon role.** Rejected above on scope, not on merit: it is the only channel that actually reaches an operator who is not looking at the Host. Revisit when the roadmap's shared alerting exists, and drive it from the latch rather than from a second journal reader.

- **A long-running `journalctl --follow` watcher with `Restart=always`.** Rejected. It buys real-time detection against a hazard measured in weeks, and pays for it with a process that can die between restarts and a supervision question the timer does not have. `Persistent=true` on a timer additionally catches up a tick the Host was down for, which a follow cannot.

- **Surface it through the CLI (`auberge bichon verify`, over ssh, on the Backup Verdict pattern).** Rejected for this issue, not on principle — it is the largest diff (Rust, tests, docs, a new subcommand) and it is pull-only, so it answers "is anything wrong?" only when asked. The latch already writes a file an ssh one-liner can read, so this stays available as a later addition rather than a prerequisite.

- **Re-emit at a higher priority with `systemd-cat` and leave it in the journal.** Rejected: it moves the line without making anything notice it, which is the status quo with extra steps.

- **Watch the folder status Bichon also sets (`UID validity changed, rebuilding...`).** Rejected: it lives in the Internal Store and is transient — overwritten when the rebuild finishes — so reading it means polling Bichon's API and racing the thing being detected. The journal line is append-only and outlives the event.

- **Alert on the rebuild by diffing envelope counts.** Rejected: it measures the consequence instead of the trigger, cannot distinguish a purge from an ordinary expunge, and needs a baseline that the purge itself destroys.

## Consequences

**Positive:**

- ADR-0012's last open gap is closed. A purge is announced within the hour instead of at the next restore, "possibly years later."
- Zero new config: no Key Registry entry, no credential, no egress. The alert cannot fail because a token expired.
- The latched report names the folder and the timestamp, which is what the restore in ADR-0012 needs as input.
- A run that cannot read the journal is distinguishable from a run that found nothing (`2` vs `0`), so the watch cannot fail silently.
- Composes with a future notifier: one file, one unit state, no second journal reader.

**Negative:**

- **Passive.** Nothing reaches the operator's phone. An operator who never runs `systemctl --failed` and never opens Cockpit learns of a rebuild no sooner than before. This is the cost of the scope decision above, and it is the reason to revisit push once shared alerting exists.
- Detection latency is up to an hour, plus however long until the operator looks.
- Journal retention bounds what the first run can see. A rebuild older than the retained journal is undetectable, and no baseline records that.
- The grep target is upstream prose. It has survived one whole-workspace refactor, but a reworded log line silently disables the watch — the failure mode is a unit that keeps passing. Nothing detects that; a canary would need a rebuild to test against.
- One more unit and timer on the Host, and a state directory that is not backed up (correctly — it is derived from the journal, and the latch is a notice, not a record of mail).

## References

- ADR-0012 — Email Archive splits immutable bodies from mutable metadata. Its Consequences name this follow-up; its restore script is what a latched rebuild sends the operator to.
- ADR-0007 §1 — the silent-vs-loud asymmetry. It decides the `--grep` question and the "no baseline seeding" question the same way.
- ADR-0006 — the Internal Store is deliberately not backed up, which is why a purge costs searchability rather than mail.
- `meta/roadmap.md` — "Alerting (via email or webhook)" under Infrastructure, still unbuilt; the reason this alert ships without a delivery channel.
- CONTEXT.md — defines **Rebuild Latch**, **Internal Store**, **Email Archive**, **Upstream Mailbox**, **Backup Verdict**.
- Bichon trigger chain: `flow.rs` UIDVALIDITY comparison → `info!` → `rebuild_mailbox_cache` deletes envelopes and blobs → refetch.
