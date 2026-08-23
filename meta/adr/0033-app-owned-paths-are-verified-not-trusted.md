# ADR-0033: A Recipe path the App owns is verified against the App, not trusted from the role

## Status

Accepted, 2026-08-23. **Applies ADR-0027's thesis to a path rather than a version** — read it from the thing itself, never from a note the role wrote. **Complements ADR-0001**: `attests:` is a second declarative command field beside `post_restore_command`, so Recipes stay data.

## Decision

A Backup Recipe may declare a **Path Attestation** — `attests:`, a command whose stdout lines are the paths the App itself reports its data lives at:

```yaml
backup:
  paths: [/srv/grimmory, /srv/books]
  attests: sudo mariadb -N -B -e 'select path from library_path' grimmory
```

`backup create` runs it **first**, before any service is stopped and before anything is staged. Every attested path must sit within a declared path, at a directory boundary — so `/srv/books` does not hold `/srv/books-archive`. Two ways to fail, both fatal for that App: `session.rs` records the failure and continues to the next. **Not for the whole run, with one exception** — the emergency backup a cross-host restore takes first escalates any App's failure into a failed step, which is why the third candidate failure mode below was dropped.

- an attested path no declared path contains;
- an attestation that exits non-zero.

**An attestation that reports nothing is not a failure.** Measured against the live host: mariadb exits `1` on a missing table and `1` on a syntax error, and returns exit `0` with empty stdout only when the table exists and holds no rows. Every way of failing to _ask_ is therefore already caught by the exit code, and emptiness only ever means the App holds no data yet — the normal state of a freshly deployed App.

Opt-in per Recipe. The twelve Apps whose data location is a config file ansible writes declare no attestation and are untouched.

## Why

Grimmory keeps its library root in its own MariaDB (`library_path.path`), written by the BookDrop UI. Nothing in the repo can see that row.

On 2026-08-23 the library was created directly in the database with `library_path` = `/srv/books`, and 138 files (134 epub, 192M) were moved there. The Recipe #614 had shipped the day before — written when the library did not exist — declared `paths: [/srv/grimmory]`. `/srv/grimmory` holds 28M of derived data: `images/` (137 cover thumbnails), `icons/`, `bookdrop_temp/`. So the Recipe covered the metadata database and every regenerable artifact, and **zero book files**. A restore would have completed green and left 138 `book_file` rows pointing at nothing.

The same divergence had a second consequence nobody had looked for. The unit sets `ProtectSystem=strict`, and its `ReadWritePaths` named `/opt/grimmory`, `/srv/grimmory`, `/srv/bookdrop`. Reading grimmory's own mount namespace confirms it: `/` is `ro`, with rw binds for exactly those three. `/srv/books` was **read-only to the service that owns it** — the initial 138 books landed only because an operator moved them by hand, and the first BookDrop review-and-import would have failed with `EROFS`. One undeclared path, two unrelated failures, neither visible from the repo.

That is the shape ADR-0027 was written against, one layer up: a note the role wrote diverges from reality exactly when it matters. Declaring `grimmory_library_path: /srv/books` fixes today and buys nothing tomorrow — re-pointing the library in the UI is two clicks, and it would silently restore the whole of #621 with every guard in this repo still green.

## Considered alternatives

- **Make an empty attestation fatal.** Taken first, on the reasoning that a table an upstream release renamed would read as an App holding no data. Measurement refutes the premise: a renamed table exits `1`, so the exit-code branch already covers it, and emptiness carries no signal about whether the question was asked. Worse, the rule broke the operation this decision most needs to survive. A cross-host restore takes a pre-migration emergency backup of the **target** host first (ADR-0026), where a freshly deployed grimmory legitimately has no `library_path` rows — and `backup.rs` escalates any App's failure into a failed emergency backup, after which the operator is asked `Continue without emergency backup? This is DANGEROUS!` defaulting to no. The first grimmory migration would have aborted by default.
- **Declare and accept the drift** — `grimmory_library_path` in defaults, a human vouches, nothing checks. This is ADR-0028's _declared_ regime, and it is a real option: 0028 argues that declaring a blind spot is what separates a reviewed clearance from an unnoticed one. Rejected because the check is one SQL query against a database the Recipe already dumps. 0028 declares where the repo _cannot_ know; here it can, for the cost of one field.
- **Extend the Backup Verdict.** Where ADR-0026 assigns completeness. Rejected on seam, not on principle: `backup verify` is repository-side — `restic snapshots`, reachability, `contains_app`, freshness — and holds no session on the App Host. Giving it a host dimension to catch this would also catch it only _after_ the incomplete snapshot exists, on the next scheduled run. Re-judging existing snapshots is a real want and a different question. It also keeps the word "coverage" free: it already names bichon's Message-ID gate and the Upstream Mailbox question the Verdict declines, which is why this concept is an _attestation_ instead.
- **`ansible.builtin.assert` at deploy.** Lives next to the declaration it checks, which is its whole appeal. Rejected: a deploy converging while the database says something else is not a broken deploy, and the library can be re-pointed at any moment between two deploys. Backups run nightly; deploys do not.
- **A latched watch on the Host**, ADR-0014 style. The right shape for a condition that is expensive to check or advisory to act on. This one is neither: it costs a query, and the correct response is to not take the snapshot.
- **Make ansible authoritative** — have the role write the `library_path` row. Rejected: it buys correctness by breaching ADR-0007's boundary that reconcile is the only App-internal state auberge writes, and trades a small blind spot for a large precedent.
- **Reconcile the other way** — move the 192M into `/srv/grimmory`, already declared, already writable, already covered, and rewrite 138 `book_file` rows. Then #621's fix is zero lines. Rejected: two directories with different lifetimes is the correct model, not an accident. `/srv/grimmory` holds a rebuildable cache; `/srv/books` holds files that exist nowhere else. Collapsing them puts the derived and the original under one path — the conflation ADR-0031 spent its Why untangling for bichon.

## Consequences

**Positive:**

- The failure #621 describes cannot be re-introduced by a UI action. Re-pointing the library fails the next `backup create` for grimmory, naming the offending path and the declared set.
- Failing before the quiesce means a false positive costs one skipped App in one run — no service bounce, and no known-incomplete snapshot for someone to restore from later when only the exit code has been forgotten.
- The grammar generalizes to any App that owns its data location. Grimmory is the only one today; the field is there when the next one arrives.

**Negative:**

- A Recipe now carries a shell command that must stay correct across upstream releases. `library_path` is grimmory's schema, not auberge's, and a rename in 2.4.0 fails the backup — the intended trade, paid as a failed nightly on the release that renames it. A table kept but no longer populated is the residual blind spot: valid SQL, no rows, silently fine.
- The check costs one SSH round-trip per App declaring it, before the quiesce window opens.
- Snapshots taken before this change lack `/srv/books`, and restore rsyncs every declared path from the stage (ADR-0026), so restoring a pre-#621 grimmory stage fails at the missing path. Not fail-_fast_: `restore` stops services before its rsync loop, so the failure lands after the quiesce, unlike the create-side check this ADR adds. The remedy is a fresh snapshot, not tolerance in the restore.

**On the `ReadWritePaths` invariant, corrected:**

`tests/grimmory_role.rs` also asserts that every directory the grimmory role creates for the service user — bar the install path — appears in the unit's `ReadWritePaths`. An earlier draft of this ADR claimed that statement holds fleet-wide and was fenced for one role, which would be the very shape ADR-0028 exists to kill. A scan of all 17 `ProtectSystem=strict` units refutes it: 13 role-created, service-owned directories are unwritable by their unit, and **every one is correct**. `colporteur`, `freshrss` and `tgtg` deny the service write access to its own install tree; `baikal-birthday-sync` cannot write `/opt/baikal/config`; `bichon.service` cannot write the uidvalidity watch's state directory. Least privilege, working as intended. `/srv/books` was the only genuine violation in the fleet.

So the invariant is role-specific, not a fleet truth with an unfenced remainder — which is why the install path is exempted here rather than required. Generalizing it needs the per-unit judgement `DECLARED_ROLES` was invented for (ADR-0028), tracked in #624 with the scan's findings.

## References

- Issue #621 — the gap, with the `/srv/grimmory` 28M vs `/srv/books` 192M measurement.
- Issue #624 — the fleet-wide question, and why the mechanical reading of it is wrong.
- ADR-0001 — Recipes are data; `attests:` joins `post_restore_command` as a declarative command field.
- ADR-0032 — the Recipe grammar's other extension of the same week: `systemd_services` names units of any type and its order is the quiesce order. Both add meaning to the Recipe without adding branching to it, which is the line ADR-0001 draws.
- ADR-0026 — restore restores what the staged backup holds; the emergency backup a cross-host restore takes first, and where completeness was previously assigned.
- ADR-0027 — an installed version is read from the artifact, never from a note the role wrote. Same argument, applied to a path.
- ADR-0028 — the _declared_ regime this decision declined, and the exemption shape #624 will need.
- ADR-0031 — the derived/original split, argued for bichon's store and reused here for `/srv/grimmory` vs `/srv/books`.
