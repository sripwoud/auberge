# ADR-0026: A restore restores what the staged backup holds

## Status

Accepted, 2026-08-21.

## Decision

`RecipeExecutor::restore` takes no parameter map. The paths it pushes are derived from the staged backup — the local `{host}/{timestamp}/{app}/` tree:

- the Recipe's declared `paths`, always, in declaration order;
- plus every `parameters[*].adds_paths` entry that exists under that tree, appended sorted so the plan an operator reads and the order pushed are stable across `HashMap` iteration.

A `parameters` `default` is a **create-time input only**. It selects what `backup create` collects and says nothing about what a staged backup on disk contains. A declared path missing from the tree is still attempted, and rsync fails that app's restore.

Two consequences of that, both load-bearing:

- The restore plan prints each derived path, so `--dry-run` answers "what does this backup hold" without a shell in the backup directory.
- The pre-migration emergency backup a cross-host restore takes first is created with `staged_parameters` of the incoming backup, not with parameter defaults. Its coverage must equal the restore's blast radius, because `rsync --delete` reaches every path the restore pushes.

## Why

`backup create -a navidrome --include-music` staged **19.92 GB**. `backup restore latest -a navidrome` reported success, pushed only `/var/lib/navidrome`, and left `/srv/music` at **4.0K**.

The parameter map is state that exists for the duration of one `backup create` invocation. It is not written into the staged backup, not encoded in the timestamp directory name, not carried in restic metadata. `restore_app` therefore had nothing to pass and passed `HashMap::new()`; `effective_paths` resolved `include_music` to its declared `false` and dropped the music path — for every backup, however it was taken.

The tree on disk is the record. `rsync --relative` preserves each remote path under `<app>/`, so a music-bearing backup holds `navidrome/srv/music`. "Was music backed up?" is answerable by `exists()`, and only by `exists()`.

The live gap was closed by hand during the netcup cutover — music re-synced from the store of record (the laptop's beets-managed `~/Music`) as the `ansible` user. That worked because a second copy existed. Nothing in the backup path guaranteed one.

## Considered alternatives

- **Add `--include-music` to `restore` and thread it through** (the issue's first option). Rejected. It relocates the burden to the operator's memory of a `create` invocation that may be months old and run from another machine, and when they misremember the failure is byte-for-byte the one this ADR closes: success reported, data dropped. A flag that has to agree with a directory is a second source of truth about that directory.

- **Walk the staged tree and restore every path found in it**, rather than intersecting the tree with the Recipe. Rejected, and this is the sharper reading of #572's "restore every path present in the snapshot dir". The tree does not record where the rsync'd path boundary was: `navidrome/var/lib/navidrome` is consistent with having backed up `/var`, `/var/lib`, or `/var/lib/navidrome`, and a walk has to guess. Guessing wrong pushes `/var` with `--delete`. The Recipe is the only thing that knows the boundaries, so it stays the vocabulary of restorable paths — with the consequence that deleting a `parameters` entry from a meta file orphans that path in every existing backup. Recipes are the contract (ADR-0001); removing one is a contract change, and the place to catch it is review of the meta file.

- **Write the parameter values into the staged backup as a manifest.** Rejected. A manifest restates what the directory tree already says, and it is absent from every backup taken before it ships — exactly the ones a restore reaches for. Deriving the answer needs no new file format and no migration.

- **Keep the parameter argument and derive the map at the call site.** Rejected. It leaves the wrong call expressible: the next caller can still pass an empty map, and this one did. Removing the argument makes a silent partial restore unrepresentable — the move ADR-0023 makes for `{admin_user}` by resolving at load.

- **Skip declared paths absent from the tree too, for symmetry.** Rejected. A declared path is unconditional. Its absence means the backup is broken — a `create` that failed, a partial push — and must fail loudly. Presence-detection there would reintroduce quiet partial restores through the other door.

- **Put `staged_paths` on `BackupRecipe`, beside `effective_paths`.** Rejected. The mapping from a remote path to its staged copy is `rsync --relative`'s layout, which the Recipe Executor owns and the Recipe knows nothing about; a Recipe that stats the local filesystem is no longer the pure data ADR-0001 makes it. `effective_paths` stays the create-side resolver and does not sort — its order reaches only rsync.

## Consequences

**Positive:**

- Every backup restores completely, including the ones already on disk: the derivation reads the directory, so nothing has to have been created under the new regime. The 19.92 GB navidrome backup restores its music.
- A cross-host migration carries optional data by default, which is what a migration means. The netcup cutover's manual music sync would not have been needed.
- `--dry-run` shows the path set, so a partial backup is visible before anything is overwritten rather than inferable afterwards from a size.
- Recipes load while the plan is built rather than mid-restore, so an unknown or non-backuppable app aborts before the first service is stopped, and the post-restore hints report the apps actually restored instead of the apps requested.

**Negative:**

- A navidrome restore from a music-bearing backup is now a ~20 GB rsync followed by a recursive `chown` over it, where it used to take seconds. There is no opt-out flag: restoring the database alone means restoring from a backup taken without `--include-music`.
- `rsync --delete` now applies to `/srv/music` too — target files the backup lacks are removed. That has always been restore semantics for declared paths and now extends to optional ones, which matters when restoring an old backup over a library that has since grown. The emergency backup covering the same paths is what makes it reversible, which is why its parameters are derived and not defaulted.
- A cross-host restore of a music-bearing backup now stages ~20 GB of emergency backup before it starts. Paid only when the incoming backup holds music, since the parameters are derived from it.
- Presence is coarse. A `create` that died partway through `/srv/music` leaves a directory that reads as present, and restore pushes it with `--delete`. Asserting a backup is _complete_ is the Backup Verdict's job (ADR-0007's boundary); this decision only refuses to drop what is there.

## References

- Issue #572 — reported it, including the `19.92 GB` → `4.0K` measurement. Its second fix option is the one taken, in the recipe-intersected form argued above.
- ADR-0001 — Declarative Backup Recipes. `parameters` / `adds_paths` are that grammar; this ADR changes a consumer, not the grammar.
- ADR-0023 — resolving `{admin_user}` at load so an unresolved Recipe is unrepresentable downstream. Same reasoning applied to a different late-bound input.
- ADR-0007 — the "backup is current" boundary the Backup Verdict asserts, and why completeness is not this decision's question.
- CONTEXT.md — **Recipe Executor**, **Backup Recipe**, and the _staged backup_ / restic _snapshot_ distinction under Flagged ambiguities.
