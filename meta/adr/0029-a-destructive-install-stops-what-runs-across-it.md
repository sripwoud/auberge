# ADR-0029: An install that destroys what a unit is executing stops it first

## Status

Accepted, 2026-08-22. **The third question in the #591 family.** ADR-0027 decided how a role reads what is installed; #599 decided which task the restart hangs off. This one decides what a role does when no restart can bridge the install at all. It does not close the family — #606 and #609 were a sixth and seventh notify-route instance the same day — it adds the case the notify route cannot serve.

## Decision

Where an install destroys what a running unit is executing, the role stops those units for the length of the install and starts them after it. The end-of-play restart is then not a weaker version of this — it is a second mechanism claiming the same job, and it goes.

`paperless` is the case, and the order is upstream's bare-metal order:

```
units templated → download → stop 4 → delete src/ venv/ → extract → venv → pip install
                → conf → migrate → createsuperuser → start 4
```

Two properties are what make the stop a fact rather than a precaution:

- **It carries the replacement's guard, and nothing stricter.** The stop sits inside the same version-bump block as the delete, so a deploy that changes nothing takes nothing down, and no run that replaces the tree skips the stop.
- **The unit templates are rendered before the install, so the stop needs no existence check.** On a first deploy the four units are loaded-but-inactive and stopping one is a no-op; `daemon_reload: true` on the stop is what makes systemd aware of files written moments earlier.

## Why

Paperless's install is not a file swap. The block deletes `src/` and `venv/`, unpacks a new release, rebuilds the venv, installs its dependencies, and then runs `manage.py migrate`. All four units `WorkingDirectory` into that tree and exec `venv/bin/…`; they keep running the deleted interpreter through open handles for the whole install, and the migration then hands them a schema their code does not know.

The notify #599 added bounds that window at end of play. It does not close it: `Run database migrations` and `Create admin superuser` have already run by the time handlers flush.

And the obvious tightening is unavailable. `meta: flush_handlers` right after the extract restarts the four units into a `venv/` that same block just deleted — `ExecStart={{ paperless_install_path }}/venv/bin/…` does not exist at that point, and all four units are `Restart=on-failure`, so the flush either fails the play mid-install or burns the restart budget and leaves them dead. Once the restart cannot be moved early enough, the only thing that can be moved is the stop.

### The outage is the point

A bump now takes paperless down for the length of the install — a pip build and a migration, minutes — instead of leaving it up and wrong. A failed install leaves it stopped with the play red, and that is the intended unattended state: down and reported, rather than serving pre-bump code against a migrated schema. Re-running the deploy converges, because the version marker is written last and the guard fires again.

### Interaction with the Backup Recipe

paperless's Backup Recipe stops the same four units and restarts them when it is done, which reads like a conflict and is not: the two never share a play. `auberge backup` is the Rust Recipe Executor issuing commands over ssh; `auberge deploy` runs ansible, and never invokes the Executor. A backup run concurrently with a deploy would restart units mid-install — the same hazard it has always carried, and the same reason its own order is backup → stop → work.

What the Recipe does settle is that this ordering is already the repo's answer elsewhere. Restore runs stop → rsync → `pg_restore` → `post_restore_command` → start, and paperless's `post_restore_command` is `manage.py migrate`. Two paths migrate that database and only one of them quiesced; the deploy path was the outlier.

## Considered alternatives

- **`meta: flush_handlers` after the extract.** Rejected as above: it restarts into a `venv/` that no longer exists.

- **Stop unconditionally at the head of the role.** Rejected. Most deploys of paperless change nothing, and each would cost an outage. The hazard is created by the replacement, so the stop belongs under the replacement's guard.

- **Keep the extract's notify as well as the stop.** Rejected. After the stop, `Enable and start all Paperless services` brings all four up on the new tree; a queued handler then restarts four units that came up seconds earlier, killing whatever the consumer and the task queue picked up in between. Two mechanisms for one invariant also leaves neither of them obviously load-bearing — which is what makes the next reordering safe to get wrong.

  This retires the notify on the _extract_ only. `Deploy paperless.conf` and `Deploy systemd services` keep theirs, and have to: a deploy that changes rendered config without changing the version stops nothing, and the queued restart is the only thing that applies it. The cost is that a bump which _also_ changes rendered config pays both — see Consequences.

- **`meta: flush_handlers` immediately before the start.** Would collapse that double restart: on a bump the queued config restart fires while the units are stopped, so it starts them and the start task no-ops. Not taken here. It flushes every pending handler for the Host, `Restart caddy` included, and this repo defers that one deliberately — the **Ingress Gate** judges Caddy once per play, against the whole assembled config. Trading a redundant paperless restart for an early Caddy restart is a separate decision from #604, and paperless has no Readiness Probe that needs the flush.

- **Guard the stop on a `stat` of the unit files, leaving the templates where they were.** Rejected. It answers "is there a unit to stop" honestly and makes the stop skippable in the same move, so a run can replace the tree without quiescing — and any fence then has to be taught to tolerate exactly that. Rendering the templates before the install makes the question unaskable instead.

- **A `rescue` that restarts the units after a failed install.** Rejected. Mid-install there is nothing correct to start: the tree is unpacked but the venv may be half-built and the migration half-applied. `Restart=on-failure` would crash-loop the four, and the operator would be looking at a running App serving errors instead of a stopped one.

## Consequences

**Positive:**

- `manage.py migrate` runs with nothing executing pre-bump code against it, which is the whole of #604.
- The delete of `venv/` no longer happens under four processes holding its interpreter open.
- One mechanism per invariant _for the replacement_: the window. The restart handler keeps its own, narrower job — a config change with no bump. `tests/paperless_quiesce.rs` fences the window's contents in order, and `install_notifies_restart.rs` treats a unit stopped ahead of the replacement, under the replacement's guard, as not-left-running — so removing the stop, or moving it after the extract, puts paperless back on its `REPLACING_ROLES` list and fails the build.
- Quiescing takes paperless out of the dest→unit model's reach, so it carries a declared entry under ADR-0028 — cleared rather than covered, beside colporteur's. The clearance is not a shrug: the window fence is what vouches for it, and the declaration is what makes a human say so.

**Negative:**

- A version bump is a deliberate outage of minutes. Undeclared before this change too — it was just an outage of correctness instead of availability.
- A failed install leaves paperless stopped. Loud, and the recovery is the same command that failed.
- A bump that also changes rendered config restarts the four units once more at end of play, seconds after they came up on the new tree — the config templates' notify firing on a run that had already quiesced. Paperless v3 is such a run: its `paperless.conf.j2` carries version-specific settings. Strictly better than before, where _every_ bump restarted twice (the extract's notify on top of the start), and the alternative is a config-only deploy that applies nothing.
- The window is only guaranteed on the bump path. The venv rebuild, the dependency install and the migration sit outside the version-bump block — grounded on `creates:` and on "no migrations to apply" instead — so a run that skips the block runs them with nothing stopped. What reaches that is a hand-deleted `venv/`, which the Installed Version fact does not read: the four units are already crash-looping on a missing `ExecStart`, and once pip finishes one can come up on new code ahead of the migration. Narrower than #604, named in `tests/paperless_quiesce.rs` rather than closed here, because closing it means changing what ADR-0027 grounds paperless's Installed Version on.
- The four unit names are spelled once more, in the stop's `loop`. Collapsing all four loops into one `paperless_services` default would read better and cost more: the fences resolve `loop` items only as literals, and a variable there blinds them.

## References

- Issue #604 — workers running pre-bump code across the migration.
- ADR-0027 — how a role reads what is installed. Same family, one level up.
- ADR-0028 — where a role the fence cannot prove declares its verdict. paperless is a cleared entry there, on the strength of the window this ADR decides.
- Issue #599 / PR #601 — the notify this replaces for paperless, and still the right answer everywhere the install is a swap in place.
- Issue #606 — `hermes`, the sixth swap-in-place instance, fixed with a notify hours before this. The two regimes side by side: a venv rebuilt under a live unit takes the restart, a venv deleted under four takes the window.
- `tests/paperless_quiesce.rs` — the window, in order.
- CONTEXT.md — **Quiesced Install**.
