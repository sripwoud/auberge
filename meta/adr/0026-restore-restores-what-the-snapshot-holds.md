# ADR-0026: A restore restores what the snapshot holds

## Status

Accepted, 2026-08-21.

## Decision

`RecipeExecutor::restore` takes no parameter map. The paths it pushes are derived from the staged snapshot:

- the Recipe's declared `paths`, always, in declaration order;
- plus every `parameters[*].adds_paths` entry that exists under the snapshot directory, appended sorted so the set is deterministic across `HashMap` iteration.

A `parameters` `default` is a **create-time input only** — it selects what `backup create` collects and says nothing about what a snapshot on disk contains. A declared path missing from the snapshot is still attempted, and rsync fails that app's restore.

The restore plan prints each derived path, so `--dry-run` answers "what does this snapshot hold" without a shell in the backup directory.

## Why

`backup create -a navidrome --include-music` staged **19.92 GB**. `backup restore latest -a navidrome` reported success, pushed only `/var/lib/navidrome`, and left `/srv/music` at **4.0K**.

The parameter map is state that exists for the duration of one `backup create` invocation. It is not written into the snapshot, not encoded in the timestamp directory name, and not carried in restic metadata. `restore_app` therefore had nothing to pass and passed `HashMap::new()`; `effective_paths` resolved `include_music` to its declared `false` and dropped the music path — for any snapshot, however it was taken.

The snapshot itself is the record. `rsync --relative` stages each remote path under `<app>/`, so a music-bearing snapshot holds `navidrome/srv/music` on disk. "Was music backed up?" is answerable by `exists()`, and only by `exists()`.

The live gap was closed by hand during the netcup cutover — music re-synced from the store of record (the laptop's beets-managed `~/Music`) as the `ansible` user. That worked because a second copy existed. Nothing in the backup path guaranteed one.

## Considered alternatives

- **Add `--include-music` to `restore` and thread it through** (the issue's first option). Rejected. It relocates the burden to the operator's memory of a `create` invocation that may be months old and run from another machine, and the failure mode when they misremember is byte-for-byte the one this ADR closes: success reported, data dropped. A flag that has to agree with a snapshot is a second source of truth about the snapshot.

- **Write the parameter values into the snapshot as a manifest.** Rejected. A manifest restates what the directory tree already says, and it is absent from every snapshot taken before it ships — exactly the snapshots a restore reaches for. ADR-0001 keeps Recipes pure data; deriving the answer needs no new file format and no migration.

- **Keep the parameter argument and derive the map at the call site.** Rejected. It leaves the wrong call expressible: the next caller can still pass an empty map, and this one did. Removing the argument makes the silent-partial-restore unrepresentable, the same move ADR-0023 makes for `{admin_user}` by resolving at load.

- **Skip declared paths absent from the snapshot too, for symmetry.** Rejected. A declared path is unconditional. Its absence means the snapshot is broken — a `create` that failed, a partial push — and must fail loudly. Extending presence-detection to it would reintroduce quiet partial restores through the other door.

## Consequences

**Positive:**

- Every snapshot restores completely, including the ones already on disk: the derivation reads the directory, so no snapshot needs to have been taken under the new regime. The 19.92 GB navidrome snapshot restores its music.
- A cross-host migration carries optional data by default, which is what a migration means. The netcup cutover's manual music sync would not have been needed.
- `--dry-run` shows the path set, so a partial snapshot is visible before anything is overwritten rather than inferable afterwards from a size.
- Recipes load while the plan is built rather than mid-restore, so an unknown or non-backuppable app aborts before the first service is stopped.

**Negative:**

- A navidrome restore from a music-bearing snapshot is now a ~20 GB rsync followed by a recursive `chown` over it, where it used to take seconds. There is no opt-out flag: restoring the database alone means restoring from a snapshot taken without `--include-music`.
- `rsync --delete` now applies to `/srv/music` too — target files the snapshot lacks are removed. That has always been restore semantics for declared paths; it now extends to optional ones, which matters when restoring an old snapshot over a library that has since grown.
- Presence is coarse. A `create` that died partway through `/srv/music` leaves a directory that reads as present, and restore pushes it with `--delete`. Asserting a snapshot is _complete_ is the Backup Verdict's job (ADR-0007's boundary); this decision only refuses to drop what is there.

## References

- Issue #572 — reported it, including the `19.92 GB` → `4.0K` measurement. Its second fix option is the one taken.
- ADR-0001 — Declarative Backup Recipes. `parameters` / `adds_paths` are that grammar; this ADR changes a consumer, not the grammar.
- ADR-0023 — resolving `{admin_user}` at load so an unresolved Recipe is unrepresentable downstream. Same reasoning applied to a different late-bound input.
- ADR-0007 — the "backup is current" boundary the Backup Verdict asserts, and why completeness is not this decision's question.
- CONTEXT.md — **Recipe Executor**, **Backup Recipe**.
