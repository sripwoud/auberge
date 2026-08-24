# ADR-0036: A directory a container owns is created by ansible, never maintained by it

## Status

Accepted, 2026-08-24. **Applies ADR-0027's thesis to a directory's ownership** — read it from the thing itself, never from a note the role wrote. **Completes ADR-0025's fixed shape** for a granted container exception, which specified how the compose file is vendored, pinned and wrapped in a unit, but said nothing about the host directories it bind-mounts.

## Decision

A host path the vendored compose stack bind-mounts into a container is **created** by ansible and never **maintained** by it.

- The creating task declares no `owner`, `group`, or `mode`, and is guarded on a `stat` of that path. **The omitted fields are what fix the incident**, and it is worth being precise about that: `ansible.builtin.file` with `state: directory` and no attributes does not touch an existing directory — measured, `changed=0` with the mode intact — so removing the three fields is sufficient on its own. The `stat` guard changes no behaviour on any deploy today. It earns its place by making the failure mode survivable rather than fatal: with it, a re-added `mode:` is inert on every host that already has the directory, and the task's shape states the intent that the fence enforces. First deploy creates it; every later deploy is a no-op either way.
- It notifies nothing. The bounce that would let the entrypoint re-fix the directory is not the remedy; not breaking it is.
- A directory no container owns keeps the ownership and mode the role declares. Immich's install and data directories, and its database dump directory, are ansible's alone and are unchanged.

`tests/immich_container_dirs.rs` enforces this, and derives the set rather than listing it: it reads the vendored compose file's bind mounts, resolves each `${VAR}` through the env template into a role variable and through `defaults/main.yml` into a path, intersects that with the paths the role's `file` tasks actually create, and asserts the create-only shape on the overlap. A bind mount it cannot resolve is a hard stop, not a skip. The complement is asserted too: a role-created directory outside the overlap must still declare `owner`, `group` and `mode`, so the rule cannot be over-applied into a dump directory left at the umask's 0755.

## Why

The role's database-directory task was correct exactly once. On 2026-08-24 at 06:11 UTC the first redeploy after a working install reset `/var/lib/immich/postgres` to `root:root 0700` while its interior stayed uid 999, and the web UI began 500ing on `loadServerConfig` about six minutes later with `could not open file "base/16384/17063": Permission denied` (SQLSTATE 42501). Path resolution needs search permission on every directory of the walk, so uid-999 backends lost the ability to open any relation file they were not already holding open — lazily, as each backend first faulted in a cold file, which is why the stack reported healthy for minutes.

Two measurements decided the shape of the fix, and both contradict the obvious reading.

**The entrypoint's chown is not first-boot-only.** `docker_create_db_directories` runs `chmod 00700 "$PGDATA"` and `find "$PGDATA" \! -user postgres -exec chown postgres '{}' +`, and `_main` calls it on every start, gated only on being uid 0 — not on `PG_VERSION`. So the defect is a window, bounded by the next container start. What turned it into an incident is the **absence** of a restart: the compose stack sets `restart: always`, which fires on process exit, and postgres did not exit. All four containers were `Up 13 hours` across the breakage. The window was unbounded and closed only by hand.

**The container's uid is another App's service account on the host.** `getent passwd 999` on auberge resolves to `gokapi` and `getent group 999` to `systemd-journal`. Ansible allocates system users from the top of the range downward, so 999 is simply the first one this fleet handed out.

## Considered alternatives

- **`notify: Restart immich` on the directory task.** The trap, and the reason this ADR exists rather than a three-line commit. It genuinely works — the entrypoint chowns on every start, so a bounce really does repair the reset — which means a reviewer can verify it fixes the symptom without noticing what it is. It restarts a four-container stack behind `up -d --wait` on every deploy, to undo damage ansible itself inflicted moments earlier, and it normalises a routine photo-library outage as the cost of a deploy. Repairing a self-inflicted wound on a schedule is not idempotence.
- **`owner: "999" group: "999"`**, matching the pinned image's internal uid. The issue offered this as "honest but coupled". Measurement says it is not honest: on this host it renders in `ls -l` as _the gokapi service user owns immich's postgres cluster_, and uid 999 is already allocated to an unrelated App's account, so any change in service-user creation order silently re-points the declaration at a different account. A numeric declaration that is a live collision is worse than no declaration.
- **Declare the blind spot** — ADR-0028's _declared_ regime, a human vouching in a const. Rejected because the repo can know this one. ADR-0025 already requires the compose file to be vendored in the role rather than fetched at deploy, precisely so what runs is reviewable in the diff — which makes which paths a container owns derivable from the repo. 0028 declares where the repo cannot see; here it can, by reading a file it already ships.
- **Keep `mode: "0700"` under the guard.** Appealing: the mode is the one field that is not a lie today, and the guard would stop it being re-applied to an existing cluster. Rejected. The entrypoint chmods `00700` on every start, so the mode is the container's business by the same argument as the ownership, and a declared-but-unenforced mode is the guard-vs-reality shape being removed rather than an exception to it. What the concession buys is also small: the creation window holds an empty directory, and the entrypoint tightens it before `initdb`. Leaving all three fields off is also what lets the fence be a flat rule instead of a per-field allowlist.
- **Recursively chown the tree to the container's uid.** Correct in principle, unaffordable in practice: an `O(library)` walk of a growing photo library on every deploy, to converge on a value only the image knows.
- **Join ADR-0035's service-directory classification** (#624, merged in #632 the same day as this). Rejected on seam. That scan is scoped to units setting `ProtectSystem=strict` and to service users ansible itself creates; immich's unit is a `Type=oneshot` compose wrapper with neither. The two fences partition cleanly — that one owns directories whose writer ansible can name, this one owns directories whose writer only the image knows.
- **Guard only the database directory**, as the issue proposed, and comment the upload directory as safe. It is safe, and only because immich-server runs as uid 0 in-container. Upstream supports `UID`/`GID` to run it non-root, and the day that is set the identical defect reappears one directory over with the same lazy, minutes-late symptom. Guarding both costs nothing — `root:root 0755` is what the upload directory gets at creation either way — and removes the trap instead of annotating it.

## Consequences

**Positive:**

- The reset cannot recur. Re-adding `owner`, `group`, `mode`, a `notify`, or removing the guard each fail the build with a message naming the path and the mechanism.
- The rule is checked against the compose file rather than a list, so vendoring a new upstream compose revision that bind-mounts a new host path the role creates is a failing build at authoring time.
- The check covers every `ansible.builtin.file` task naming one of these paths, not only the one that creates it, so a second task added later to _maintain_ the same directory — `owner:` with no `state:`, the shape that reintroduces the defect with the creator still clean — fails the build. A shell that chowns, and the short module form the fence cannot read, are hard stops rather than silent passes.
- Deploys stop touching the two largest directories in the fleet at all, which is also the correct posture for a multi-GiB photo library.

**Negative / accepted:**

- First creation takes the umask's mode, not a declared one — `0755` on a directory the postgres entrypoint chmods to `00700` before `initdb`. The window holds an empty directory. An image that did not chmod its own data directory would be left at `0755`; that is a per-image fact the repo cannot read, and the exact-version pin ADR-0025 requires is what bounds it.
- A `risky-file-permissions` suppression now sits on the creating task. It is the honest local record of a deliberate omission, and it is load-bearing — removing it fails lint.
- The fence models three loop shapes and hard-stops on a fourth. Loud and trivially fixed; the alternative is a fence that silently narrows to whatever it happened to parse.
- A directory both a container and ansible write would need a third regime. None exists today, and one appearing is a review-time catch.
- Immich remains the only granted container exception, so this rule has one subject. Written down because the grant procedure in ADR-0025 guarantees there will be others, and because the failure it prevents took two measurements to distinguish from the fix that looks identical.

## References

- Issue #630 — the incident, with the SQLSTATE, the `ctime` and the live remediation.
- ADR-0025 — the container grant and its fixed shape, which this completes on the directory dimension.
- ADR-0027 — an installed version is read from the artifact, never from a note the role wrote. Same argument, applied to ownership.
- ADR-0028 — the _declared_ regime this decision declined, and why a vendored compose file puts this inside the repo's reach.
- ADR-0033 — the sibling application to a path rather than a version, and the warning against dressing a role-specific invariant as a fleet claim.
- ADR-0035 (#624) — the service-directory classification this deliberately does not join, and why the seam falls where it does. Scoped to `ProtectSystem=strict` units and ansible-created service users, so it cannot see immich by construction.
