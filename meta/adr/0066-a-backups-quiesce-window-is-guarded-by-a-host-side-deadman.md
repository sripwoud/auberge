# ADR-0066: A quiesce window is guarded by a host-side deadman, not the driver alone

## Status

Accepted, 2026-09-01. Decided in #774's triage grilling, implemented in #774 for `backup` and amended in #775 to cover `restore` as well; extends ADR-0032's quiesce order to a case it does not cover — the driver process itself dying mid-window.

## Decision

**The Recipe Executor arms a `systemd-run --on-active` timer on the Host itself before stopping a Recipe's `systemd_services`, and re-arms it at every step boundary rather than covering the whole operation with one fixed span**: for a backup, stop → dump → each rsync → restart; for a restore, stop → each rsync → each chown → the database restore → the post-restore command → restart. What is bounded is "how long can any one step run before the driver is presumed gone" — one hour, picked with no historical per-step timing to size it against, deliberately generous — not total backup duration, which varies too much by App and by network to fix sensibly in one number.

On fire, the armed unit runs `reset-failed` then `start` — **never** `restart` or `stop` — against the same units, in the Backup Recipe's declared quiesce order (ADR-0032). `reset-failed` is required because a unit stopped by `SIGTERM` and exiting nonzero (granian, on paperless-webserver) latches `failed`, and `Restart=on-failure` does not self-heal a unit that was stopped deliberately. Start-only makes a fire racing a still-alive driver a no-op rather than a collision — `start` on an already-active unit does nothing.

Scoped per-App: each `RecipeExecutor::backup` and `RecipeExecutor::restore` call arms and disarms its own timer, matching the granularity the executor already restarts at. One transient unit name per App, shared between the two operations — they can never overlap for one App, because a cross-host restore's emergency backup disarms before the restore phase arms.

A fire is not silent: it leaves a marker the _next_ guarded run against the same App reads and reports (warn, not hard-fail — a missed window failing the next scheduled run too would cascade one outage into two). The marker is one line, `<operation> <UTC timestamp>`, because the two operations leave the App in different states: an interrupted backup means the snapshot may be incomplete, an interrupted restore means the App itself may be half-overwritten and the emergency backup is the way back. A marker naming no operation — one written before #775 — still warns.

## Why

`RecipeExecutor::backup`'s `Err`-path restart is careful — every failure branch routes through a restart before returning — but that guarantee is a property of the Rust process, and both real outages happened because the _process_ died, not because it hit an `Err` branch: once by laptop suspend under `upowerd`'s critical-battery action, once by a Ctrl-C that `src/signal.rs` sends straight to `exit(130)` with no restart at all. No amount of in-process error handling reaches either case, because the code that would run it never executes.

### Why heartbeat, not one fixed timeout

A single timeout covering the whole operation has to be sized for the slowest step of the slowest App, which makes it either too loose to be a real safety net (the incident's own media rsync could plausibly run for tens of minutes) or too tight for an App that grows past what it was sized for. Re-arming at each step bounds the smaller, more stable question of "how long can one step legitimately take," and gives the same one-hour margin real headroom on every step rather than needing headroom sized to the sum of all of them.

### Why accept the Quiesced Install collision, not interlock

A deploy's own quiesce window (ADR-0029, e.g. paperless's venv rebuild) could in principle overlap with an armed deadman and get restarted mid-install. ADR-0029 already accepts this exact class of hazard between backup and deploy — "the two never share a play" is a fact about how they're invoked, not a guarantee enforced by any lock — so a deadman interlock would be new cross-mechanism machinery closing a seam ADR-0029 already leaves open by design. The heartbeat model bounds the exposure to one step's duration rather than the whole operation, which is the mitigation taken here instead of a lock; a real interlock is a reasonable escalation if this is ever hit in practice, not built pre-emptively.

## What it costs

- **The one-hour interval is a judgement, not a measurement.** No per-app or per-step timing is recorded anywhere in the codebase to size it against — the first real number this repo will have is whatever the marker file's own fire history accumulates.
- **The Host, not just the driver, now needs a way to run a detached command.** No `SshSession` method returns without waiting on the remote process today; arming the timer needs one shaped for that, alongside the existing blocking ones.
- **A deploy running concurrently with an armed deadman can still be restarted into mid-install.** Accepted per ADR-0029's existing stance, not newly introduced by this decision — but this is the first mechanism to independently _act_ on a Host during a window ADR-0029 assumed only Ansible would touch.
- **A deliberately abandoned restore is now restarted too.** Ctrl-C mid-apply is indistinguishable from a driver that died, so the Host brings the App back up within the hour on whatever half-restored state the abandoned run left behind. Start-only recovery cannot tell the two apart; the marker's restore wording is what tells the operator afterwards (#775).

## Alternatives considered

- **Interlock with the Quiesced Install window** (a shared lock/flag deploy sets and the deadman checks). Rejected for now: real, but separable machinery closing a seam ADR-0029 already accepts. Revisit if the residual window is ever actually hit.
- **A fixed timeout over the whole operation instead of a heartbeat.** Rejected: has to be sized for the slowest step of the slowest App, which is either too loose or too tight depending on which App grows fastest.
- **An active push (webhook/`ntfy`) on fire, instead of a marker file the next run reads.** Rejected for v1: no new external dependency, and it mirrors how this incident was actually discovered (stale staging dirs, offsite snapshot gaps) — the next run already has the right vantage point. Worth revisiting if unattended detection latency matters more than it does today.
- **A restart, not start-only, on fire.** Rejected: loses the fail-safe property that a fire racing a live driver is a no-op — `restart` would interrupt units the driver just brought back up correctly.

## References

- Issue #774 — the two real outages (36h, 11h) and the original proposal.
- Issue #775 — restore's identical quiesce gap, closed by widening this decision to it rather than superseding it.
- Issue #776 — `Host.become_method` found dead while researching this, unrelated.
- ADR-0029 — the Quiesced Install window this accepts overlap with rather than interlocking against.
- ADR-0032 — the quiesce order this replays on fire.
- CONTEXT.md — **Deadman Timer**.
