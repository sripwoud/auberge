# ADR-0027: An installed version is read from the artifact, never from a note the role wrote

## Status

Accepted, 2026-08-22. **Complements ADR-0017** — that ADR decided _which_ version a Playbook installs. This one decides how a role answers whether that version is already installed.

Amended 2026-08-25 (ADR-0037, #634): radio retired its version pin and its `package_facts` read with it — apt's `state: present` owns the idempotence — and leaves the Artifact-read examples below.

## Decision

A role decides whether to install by reading what is on the Host, not what it once recorded about the Host.

Three regimes, in order of preference:

- **Versioned dest.** The artifact's own path carries the version, so a bump is a dest that cannot already exist and the guard is a bare `stat` of it — no marker to reconcile, and the download always notifies its restart handler. Grimmory (#597). Requires the install to be a single file the role names.
- **Artifact-read.** Interrogate the installed thing: `package_facts` where apt owns it (`navidrome`), the binary's own `--version` (`blocky`, `hermes`, `tgtg`), or a file the release itself ships (`baikal` slurps `BAIKAL_VERSION` out of `Core/Distrib.php`). A deleted, downgraded or hand-swapped artifact is visible.
- **Marker-plus-stat.** Where neither is available, a sidecar `version` file the role writes is permitted — **and-ed with a `stat` of the artifact**, so an absent artifact reads as nothing installed. Five roles: `bichon`, `colporteur`, `gokapi`, `headscale`, `paperless`.

The grounding goes into the `<role>_installed_version` fact, not into the guards that read it. `colporteur` gates six tasks on that fact and `gokapi` five; a compound condition repeated at each site is six chances to diverge, and the fact is the one place every guard already passes through.

The artifact is stated once, in `defaults/main.yml`, and the stat, the install and the unit's `ExecStart` all resolve through it — `bichon_binary_path`, `colporteur_binary_path`, `gokapi_binary_path`, `headscale_binary_path`, `paperless_src_dir`.

`tests/install_guards.rs` enforces this. It detects the marker regime structurally — a role that slurps a path it also writes from inline `content:` is reading its own note — and fails the build when such a fact consults no stat of a path the role did not write.

## Why

A marker records that the role once installed a version. It cannot answer whether the artifact is still there, and the two questions diverge exactly when it matters: during recovery.

Grimmory 2.3.0 shipped a release asset with no frontend assets bundled (#586), and the fix required replacing the jar on the Host. Deleting `/opt/grimmory/app.jar` and re-running the deploy is the obvious recovery path — and it silently did nothing. The marker still said `2.3.0`, the download's `when` was satisfied, ansible reported the role converged, and the unit crash-looped on `Unable to access jarfile`. Two deploys were spent before the cause was the guard rather than the artifact.

The same shape was in five more roles (#596). All five download an archive and unpack it into an install path whose executable is the unit's `ExecStart` target; none of them looked at that executable.

What makes this a decision rather than six bug fixes is that the repo had already solved it twice over. Six other roles were on the artifact-read regime and were structurally immune — not by policy, just by what was convenient for each. Writing the preference down is what makes the immune majority the rule and the marker the documented exception.

### Why these five cannot take the versioned dest

Grimmory's fix is better than anything a marker can do, and it is not portable to these five as they are laid out. It works because the install is one file whose name the role chooses: stamping the version into it makes every bump a fresh dest, which is what retires the marker.

These five unarchive into a fixed layout. The archive decides the filename — `bichon-server`, `colporteur`, `gokapi`, a `src/` tree — so versioning the dest means versioning the _directory_ and moving the unit's `ExecStart`, the ownership, and a story for reaping the old tree with it. That is a per-role restructure, not this change. `headscale` is the exception worth revisiting: it downloads a single binary and copies it to a path the role names, so it could follow #597 directly.

### Why not artifact-read now

Artifact-read is available to all five and turns an unverifiable claim into a deploy-time string equality: `gokapi --version` must print something that matches the pinned literal exactly, or the role reinstalls on every deploy forever. That format is not knowable from the repo, only from the Host, and getting it wrong is a regression across five Apps at once. Marker-plus-stat is decidable entirely from the repo and closes the incident; converging a role is a per-role follow-up once its output is verified live.

### Why `paperless` watches `src/manage.py`

`paperless` has no single installed file: the tarball lands a source tree and the venv is built separately by two later tasks that are already grounded (`creates:`). Its four units all set `WorkingDirectory=<install>/src` and the consumer execs `manage.py` from there, so that file's absence is what means the tree has to come back — and the units' `venv/bin/…` would be the wrong sentinel for a missing _source_ tree. Firing the guard also deletes and rebuilds the venv, which is the correct recovery for a re-extracted source tree.

## Considered alternatives

- **Or the stat into each guard**, as `grimmory` did in #592. Rejected at scale. It works for one guarded task and repeats a compound boolean six times in `colporteur`, five in `gokapi`, four in `headscale` — 21 sites across the five. Grounding the fact instead puts it in one place per role and leaves every guard as it was.

- **A dedicated `<role>_install_needed` boolean fact**, guards reading that. Rejected: it introduces a variable to say what `<role>_installed_version` already says. "Nothing is installed" is the honest reading of an absent artifact, and every existing guard already spells the comparison.

- **An allowlist of marker roles in the test**, as `version_annotations.rs` keeps `TOOL_VERSIONS`. Rejected for the fence: an allowlist only fences roles someone remembered to list, and the failure mode being fenced is a _new_ role written in the old shape. Structural detection needs no per-role maintenance -- it recognises a marker written by `copy` with inline `content:` or by `template`, which is every marker the repo has -- and it earned that on its first day: run after #597 it dropped `grimmory` on its own, because a role with no marker left is no longer in the regime.

- **Drop the marker and reinstall unconditionally.** Rejected: it re-downloads and restarts every App on every deploy.

- **Stat the downloaded archive in `/tmp`.** Rejected. It is the wrong artifact — every one of these roles deletes the tarball after unpacking, and a `/tmp` path is not what any unit execs. Related but distinct: `get_url`'s `force: false` is a conditional GET, not a skip-if-exists, which is what #595 was.

## Consequences

**Positive:**

- Deleting the artifact is a working recovery path in all five roles. It is the first thing an operator tries and it now does what it looks like it does.
- A restore that brings back an install path without its sidecar reinstalls instead of being assumed current.
- One shape across the five, and the artifact path stated once per role, so the stat, the install and the unit cannot drift apart.
- The fence makes the next filing of this bug a failing build instead of an incident. #591, #594, #595 and #596 are one family: a guard that tests a record of intent instead of the thing it guards.

**Negative:**

- A `stat` answers existence, not identity. An artifact present at the wrong version — the #586 workaround of hand-copying a known-good jar of the same version, a partially-written download, a corrupt unpack — is invisible to marker-plus-stat, and is exactly what the other two regimes catch. This one closes the absent case only.
- `paperless`'s sentinel is a path inside an upstream layout. If a release relocates `src/manage.py`, the role reinstalls on every deploy until the sentinel is updated. Loud rather than silent — every deploy reports changed — and version bumps are reviewed.
- Five roles now do one extra `stat` per deploy. Unmeasurable.
- A sixth marker role must be declared in `MARKER_ROLES`, which is deliberate friction: it is what subjects the role to the sentinel, unit-grounding and scenario assertions.

## References

- ADR-0017 — Version Resolution: which version is installed. This ADR answers whether it already is.
- CONTEXT.md — **Installed Version**, and the invariant `tests/install_guards.rs` enforces.
- Issue #591 / PR #592 — `grimmory`, where this was first found and first fixed, in the shape this ADR supersedes.
- Issue #595 / PR #597 — retired grimmory's marker outright by versioning the jar filename. The preferred regime, and why grimmory is not in the list above.
- Issue #596 — the same guard in `bichon`, `colporteur`, `gokapi`, `headscale`, `paperless`.
- Issue #586 — the broken 2.3.0 release asset whose recovery path the guard blocked.
