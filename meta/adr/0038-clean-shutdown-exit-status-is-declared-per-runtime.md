# ADR-0038: A clean shutdown's exit status is declared per runtime

## Status

Accepted, 2026-08-25. **Applies ADR-0028's declared regime to shutdown exit statuses** — the third fact about a unit the repo cannot read off its template, after restart edges (ADR-0028) and directory writability (ADR-0035).

## Decision

`tests/shutdown_exit_status.rs` computes, for every `.service` a role installs — 26 of them, templated or copied, under `/etc/systemd/system` or a user's `~/.config/systemd/user`, drop-ins excluded because the `ExecStart` they would be judged against is not theirs — the runtime it execs: `ExecStart`'s argv[0] by basename, resolved against the role's defaults.

`CLEAN_SHUTDOWN_EXITS` declares, per runtime, the status a clean shutdown leaves behind and why. One entry today: `java`, 143, because the JVM installs a SIGTERM handler, runs its shutdown hooks, and then exits 128+SIGTERM itself rather than dying from the signal.

The pairing is asserted in both directions, and by exact value:

- a unit whose runtime is declared must carry `SuccessExitStatus=<status>`;
- a unit carrying `SuccessExitStatus=` must exec a declared runtime, and must forgive **exactly** that one status — a wider whitelist fails;
- a unit carrying `SuccessExitStatus=` must not also carry `Restart=on-failure`, because the two cancel out for the forgiven status (below);
- a declared runtime no unit execs any more fails, so the table cannot outlive the App that needed it.

The scan's own reach is fenced as a set — `FLEET_SERVICES`, `<role>/<unit>` — the same way ADR-0035 fences its directories, and for the same reason: a count stays green when one unit replaces another, and cannot name which one moved. Three further reads are hard stops rather than silent misreads: a systemd exec prefix (`-`, `@`, `+`, `!`), a unit declaring more than one `ExecStart`, and a unit name that does not resolve — the last because a var-driven `loop:` the scan cannot expand would otherwise fail the `.service` test and drop out of the domain unseen, which is the one path by which a new unit could enter the fleet without entering the fence.

## Why

systemd is lenient about a service _dying from_ SIGTERM: `code=killed, signal=TERM` scores as success with nothing declared. It is not lenient about a service that _catches_ SIGTERM, shuts itself down, and then exits 128+15 of its own accord — that arrives as `code=exited, status=143` and scores as a failure.

grimmory is a Spring Boot app, so it does the second thing, and the Backup Recipe stops it nightly (ADR-0001). Every snapshot window therefore latched `grimmory.service` into `failed` — eight stop-correlated occurrences in the five days of journal auberge holds, one more on vieille-auberge — with the graceful-shutdown log lines sitting directly above the verdict. Nothing was broken; what was lost is that `systemctl --failed` is the fleet's health signal, and a nightly false positive in it costs exactly what the signal is for.

Keyed on the runtime rather than on the unit because that is where the fact lives. Whether SIGTERM ends in an exit or a signal death is a property of the language runtime, not of the App: a fleet-wide scan of both hosts' journals found the 143 signature on grimmory and nowhere else, and every other runtime the fleet execs — Go (blocky, gokapi, headscale, bichon), Rust (colporteur), Python (paperless, tgtg, hermes, baikal's two sync oneshots), PHP (freshrss), Node (actual), liquidsoap, caddy, docker — dies from the signal. grimmory is the only JVM App today; keying on `java` means the next one is fenced without anyone remembering to enrol it.

Exactly one status, because the whole value of the change is that the graceful path stays distinguishable from a crash — the ticket's third acceptance criterion. grimmory's own record shows the distinction has work to do: 4628 `status=1` exits inside one seven-hour window on 2026-08-22, and `-XX:+ExitOnOutOfMemoryError` in its JVM opts, which is a deliberate nonzero exit the unit must keep restarting on. A cgroup OOM kill (`MemoryMax`, ADR-0021) arrives as a signal death, so it stays a failure by construction. What the fence proves here is that the declaration is narrow; the scoring is systemd's, and no test in this repo exercises it.

And `Restart=on-failure` has to go with it, which is not obvious and was nearly missed. Forgiving a status removes it from `on-failure`'s trigger set: grimmory under `on-failure` recovered from an operator's `kill` in five seconds, and with 143 forgiven it would instead sit `inactive (dead)` — clean, unreported, invisible to `systemctl --failed` as well as to the restart. Repairing the health signal in one direction while losing it in another is not a repair. `Restart=always` recovers from a SIGTERM nobody meant and still honours a deliberate stop, because systemd suppresses restart after an explicit `systemctl stop` — the Backup Recipe's window is unaffected. The fence carries the coupling so the pair cannot be split later: forgiving a status while asking `on-failure` to watch for it fails the build.

## Considered alternatives

- **`SuccessExitStatus=SIGTERM`**, the form that reads like it says what is meant. Rejected because it says something else: signal names in `SuccessExitStatus` forgive _signal deaths_, and the JVM's shutdown is an _exit_. It would have forgiven the one case systemd already forgives and left the real one failing.
- **Make the App exit 0 on SIGTERM.** grimmory's jar is built in this repo, so this was available. Rejected on layering: 143 is the JVM's correct convention, not a defect to patch out, and fixing it in one App leaves the next JVM App in the same trap while making its unit's scoring depend on an application detail no operator reading the unit can see.
- **Declare per unit** — a list of units that need `SuccessExitStatus`. Rejected for ADR-0028's own reason: nothing computes which units belong on the list, so a new JVM App enters no fence at all. The runtime is the computable key.
- **Keep `Restart=on-failure`** and accept that an external SIGTERM no longer restarts, on the reading that a human who kills a process meant it. Rejected: the unit then goes down silently, which costs the same signal this ADR exists to protect. `Restart=always` is the adopted complement, not a competing alternative — an earlier draft of this ADR had it the other way round.
- **Stop reading `systemctl --failed`.** Rejected: gives up the signal instead of repairing it.
- **Scan the wrapper scripts units exec, not just argv[0].** Deferred, not rejected. Four units exec a shell script (`lego-renew.sh`, `vibecoder-start.sh`, immich's backup script, bichon's archive script) and none of them runs a JVM. Following argv[0] into a script is real work for a case the fleet does not have; the boundary is recorded below instead of guessed at.

## Consequences

**Positive:**

- The build now fails on: a unit whose runtime exits nonzero on a clean shutdown and does not say so, a `SuccessExitStatus` wider than that one status, a `SuccessExitStatus` paired with `Restart=on-failure`, a `SuccessExitStatus` on a unit whose runtime nothing vouches for, a declared runtime the fleet no longer execs, a service the scan finds that `FLEET_SERVICES` does not list or lists and no longer finds, and an exec prefix, a second `ExecStart`, or an unresolved unit name the model does not read. Each failure names the runtime and its why.
- `systemctl --failed` becomes worth reading again: the nightly latch is gone, and so is the unrelated `apache2.service` latch the same audit turned up (php pulls apache2 in, its postinst fails against caddy's port, and the purge that follows removed the unit file while leaving the failure — fixed in the same change).
- A deliberate SIGTERM from anywhere — an operator's `kill`, not only systemd's stop — now scores as the deliberate stop it is.

**Negative:**

- The model reads argv[0] and nothing deeper. A unit that execs a wrapper which execs a JVM is outside it, silently. Recorded as the deferred alternative above rather than papered over.
- A runtime's exit convention is declared, not measured. The fence proves the pairing between a runtime and a `SuccessExitStatus`; it cannot prove the status is the right one. A JVM that someday exits 0 on SIGTERM would make the declaration wrong with nothing to catch it — the defining limit of the declared regime, not a defect in this instance of it.
- `FLEET_SERVICES` binds to role and unit names, so adding, removing or renaming any unit fails this test until the list moves with it. Loud and cheap, the trade ADR-0028 already accepted.
- The reach fence can only pin what the scan reads. A unit installed somewhere the model does not look — `/usr/local/lib/systemd/system`, a dest built by a module other than `template` or `copy` — is absent from both sides of the equality and so fails nothing. The fleet installs units in exactly two directories today; the fail-loud on unresolved names closes the one case that could have slipped in unnoticed, and this one is recorded rather than closed.

## References

- Issue #635 — the nightly false positive, and the fleet audit that found it isolated to one unit.
- ADR-0028 — the declared regime: computed key, per-entry why, equality in both directions.
- ADR-0035 — the same regime applied to directories, and the precedent for pinning a computed domain by set equality rather than by count.
- ADR-0001 — the Backup Recipe stops units, which is what makes a graceful shutdown a nightly event rather than a rare one.
- ADR-0021 — Memory Budgets; a `MemoryMax` kill is a signal death, so it stays a failure without being declared.
