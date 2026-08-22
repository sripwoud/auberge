# ADR-0028: A restart the fence cannot prove is declared, not documented

## Status

Accepted, 2026-08-22. **Complements the #601 invariant** — every install that names an App Version notifies the restart of what runs the artifact it replaced. That invariant says what a bump must do; this ADR decides how it is enforced where the fence's model cannot see.

## Decision

Two regimes for the restart-on-replace invariant, split by what the repo can know:

- **Proven.** `tests/install_notifies_restart.rs` follows a version-bump install's `dest` into a unit the role templates under `/etc/systemd/system` and computes which restarts are owed. Preferred wherever it works; `REPLACING_ROLES` asserts its coverage by equality.
- **Declared.** Where what runs the artifact is outside that model's reach, a human vouches for the wiring in `DECLARED_ROLES`: per role, why the model cannot see it and the exact `(task, handler)` notify edges that must exist. The test asserts each declared task exists, notifies the declared handler, and that the handler actually restarts (`state: restarted`) — and it asserts the _set_ of roles needing a declaration, computed as version-bump installers minus the proven, equals the const in both directions. A new unprovable role fails the build until classified; a declaration the model has since learned to reach, or whose role is gone, fails until removed.

Three kinds are out of reach today, one per way an App can be run by a unit its role does not template: apt's php-fpm (`baikal`, `yourls`), apt's own unit for a deb the role hands it (`navidrome`), and a systemd _user_ unit execing a venv a `command` built (`hermes`). `colporteur` is declared with empty edges — the model's clearance is correct there (a timer-driven `oneshot` execs the artifact afresh at every activation), and declaring the emptiness is what separates a reviewed clearance from an unnoticed one, because from inside the repo the two are indistinguishable: clearing `baikal` wrongly and clearing `colporteur` rightly produce the same scan result.

## Why

Removing `baikal`'s or `hermes`'s `notify` was a green build (#605). Both were correct on the day the fence landed, and the only record of why the fence could not check them was a doc comment ending "nothing here would notice if either stopped" — the shape that made #596 five roles instead of one: an invariant documented but unfenced for a subset nobody re-checks.

The set being computed rather than hand-listed earned its keep on its first run: #605 enumerated the blind spot as php-fpm apps and user units, and the equality assertion immediately surfaced `navidrome` — a third kind neither the issue nor a targeted search had found. Manual enumeration of a blind spot undercounts by exactly the amount you cannot see; that is what makes it a blind spot.

`yourls` was the one live-ish gap: its checkout notified nothing, tolerable only because PHP re-reads sources per request. The php-fpm restart is cheap, so the notify was added on both git tasks rather than baking an opcache assumption into the fence as a per-role exemption.

## Considered alternatives

- **An existential obligation** (#605 option A): an install naming the App Version must notify _at least one_ restart handler, unit unknown. Rejected twice over. `baikal` also owns `Restart caddy`, so the check passes with the notify rewired to a handler that never touches php-fpm — the wrong-handler hole. And `hermes`'s carrying task (`Install hermes-agent into venv`) names no version at all; it is on the bump path only through a registered result of the clone, so the existential cannot find it without modeling register dataflow.
- **Model `command`-built artifacts** (option B): follow `chdir`/`creates`/argv. Rejected: more reach, much more guessing, and the guess would still stop at the venv-to-unit link.
- **Resolve units the role does not template** (option C): php-fpm's unit belongs to apt and is not in the repo, and the fact that php-fpm is what serves baikal's release exists nowhere the model can read. Any implementation would encode that fact as data — which is this decision, minus the machinery pretending it was derived.
- **Keep the doc comment.** The status quo #605 was filed against.

## Consequences

**Positive:**

- The blind spot is enumerated in one executable place instead of prose, and it can neither silently grow nor silently rot.
- Removing, or rewiring to the wrong handler, any of the five roles' restart notifies is now a failing build with a message naming the role, the task, the handler and the reason.
- The next `navidrome`-shaped discovery is a failing test at authoring time, not an incident.

**Negative:**

- Declarations bind to task-name strings; renaming a declared task breaks the test. Loud and trivially fixed, and task names are already load-bearing for `notify` itself.
- The computed net is `INSTALL_MODULES`-based. A role installing purely through `shell`/`command`, with no file-writing task anywhere on its bump path, enters neither regime unseen (`hermes` enters through its git clone; `baikal` through its download). No role has that shape today, and one that gains it is a review-time catch, not a fence-time one.
- The declared regime asserts the wiring exists, not that the human vouched correctly — a wrong declaration is fenced-in wrongness. That is the definition of the regime, not a defect the proven regime shares.

## References

- Issue #605 — the gap: the fence cannot see php-fpm apps or user units.
- Issue #601 — the invariant and the proven fence this complements.
- Issues #599, #603 / PR #606 — the failures that shaped both: an install that notified nothing.
- ADR-0027 — the sibling decision one level up: the guard on whether to install; this is the guard on what must restart when it does.
- CONTEXT.md — the **App Version** invariant list entry pointing here.
