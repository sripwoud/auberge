# ADR-0035: A service-owned directory carries a declared classification

## Status

Accepted, 2026-08-24. **Applies ADR-0028's declared regime to directories** — the per-unit judgement its Consequences section said #624 would need. **Closes ADR-0033's corrected reading**: the `ReadWritePaths` invariant is not a fleet truth, so what generalizes is not the invariant but the classification underneath it.

## Decision

`tests/service_directories.rs` computes, for every role with a `ProtectSystem=strict` unit or a Backup Recipe, the set of directories its tasks create for a service user — a strict unit's `User=`, or the `<role>_sys_user` default for Apps served by units no role templates. Each computed directory must appear in `DECLARED_DIRECTORIES`, and each declaration must still be computed: equality in both directions, so a new directory fails the build until a human classifies it and a stale declaration fails until it is removed.

A declaration states three things, each fenced:

- a **kind** — `Install` (a deploy puts it back), `Staging` (contents in flight or derived; the source of record is elsewhere), `Data` (the store of record), or `Expendable` (operational state the fleet accepts losing). The Backup Recipe must cover a directory — at a directory boundary, parameter-gated `adds_paths` included — **iff** its kind is `Data`.
- the **writers** — exactly the strict units able to write it. Every strict unit of the role is asserted against the list in both directions: a denial is declared least privilege, and a new or dropped grant fails until the declaration moves with it.
- the **why** — the sentence a maintainer reads when the fence fires.

The Recipe side carries its own declared remainder, `RECIPE_ONLY_PATHS`: a Recipe entry no directory task creates for a service user (navidrome's `/etc/navidrome`, syncthing's three files under the admin home) is vouched for by name, and a vouched entry the Recipe no longer lists fails until removed. Removing any `paths:` entry from a meta file therefore fails the build from one direction or the other.

One grant changed with the fence's arrival: grimmory's unit no longer writes `/opt/grimmory`. The jar and the rendered `.env` are ansible's to write and the service's to read — colporteur, freshrss and tgtg already denied their install trees, and the grant was the fleet's one inconsistency, left open in #624 and settled here.

## Why

#621 shipped both guards role-scoped to grimmory, and an earlier framing of #624 called the first one mechanical and fleet-applicable: every service-owned directory bar the install path belongs in `ReadWritePaths`. The scan ADR-0033 records refuted that — 13 service-owned directories across 17 strict units are unwritable by their unit, and every one is deliberate: three roles deny their install trees, baikal's two sync oneshots are each granted only their own target, bichon's server cannot touch the uidvalidity watch's state. A naive fleet guard would have failed 13 correct designs, and "fixing" them would have weakened five roles' hardening.

So the expensive thing both guards need is the same thing: a per-directory classification the roles do not state. Once it is declared, both #621 guards fall out as theorems — writability is the writers list checked against every strict unit, and Recipe coverage is the kind. And the set being computed rather than hand-listed is ADR-0028's own lesson: its first run found a third blind-spot kind (`navidrome`) that enumeration had missed; here the computed domain pulls in a new strict unit or a new Recipe on its own.

## Considered alternatives

- **Require writability of everything bar install paths, fleet-wide** — the issue's original framing. Refuted by the scan above; the fence would have demanded the weakening of five roles.
- **Keep the grimmory shape and stamp it per role** — eleven copies of `except()` with role-local exemptions. Rejected: the exemptions were already half a classification, stated negatively, and nothing computes which roles need a copy — a new role enters no fence at all.
- **Let the kind imply writability**, as the issue's own table sketched (install: no, data: yes). Rejected on two counterexamples the fleet already holds: baikal's `/opt/baikal/Specific` is `Data` no strict unit may write (apt's php-fpm owns it, outside the model), and paperless's `/opt/paperless` is `Install` all four units write (the grant covers the tree so its nested data stays reachable). Writability is a per-unit fact, so it is declared per directory as `writers` instead of inferred from the kind.
- **Assert Recipe paths equal data directories**, the #621 guard's shape. Rejected: equality was only ever true for grimmory. gokapi's one Recipe path covers three nested directories, bichon's covers two, navidrome's music library is parameter-gated — recursion is the Recipe's semantics (rsync), not drift. Containment as a biconditional keeps both failure directions without faking a set equality the fleet never had.

## Consequences

**Positive:**

- The build now fails on: a service-owned directory nobody classified, a classification for a directory no role creates, a `ReadWritePaths` grant appearing over or vanishing from any classified directory, a grant the role's defaults cannot resolve to a path (the fence fails loud rather than open), a `Data` directory the Recipe misses, a Recipe entry nothing accounts for, and a `paths:` entry removed from any meta file. Each failure names the declaration and its why. A grant over a path no role creates for a service user — freshrss's `/var/lib/php/sessions` — stays out of the model, deliberately: the fence classifies what roles create, not everything units may touch.
- The 13 denials stop being a scan result in an issue and become 13 declarations with reasons, each one a build failure away from being noticed when it stops being true.
- grimmory's service can no longer rewrite its own artifact — the fleet's install trees are now uniformly denied.

**Negative:**

- Declarations bind to resolved path strings and unit names; renaming a default or a unit fails the build until the declarations move. Loud and cheap, the trade ADR-0028 already accepted.
- A wrong `why` is fenced-in wrongness: the fence proves the facts (writers, kind-vs-Recipe), never the reasoning. That is the definition of the declared regime, not a defect.
- A unit without `ProtectSystem=strict` carries no writability fact to assert — calibre's unit, freshrss-update, everything apt owns. The fence says nothing there, and says so by construction rather than pretending.
- Every new service-owned directory now costs a human a classification. That is the point, but it is a cost.

## References

- Issue #624 — the corrected framing this ADR implements; issue #621 — where both guards were born.
- ADR-0028 — the declared regime: computed set, per-entry why, equality in both directions.
- ADR-0033 — the scan that refuted the mechanical framing, and the attestation that guards the one path the App itself owns.
- ADR-0032 — why the uidvalidity watch's state stays outside every backed-up path (`Expendable`, in this ADR's terms).
- ADR-0026 — parameter-gated paths are still paths a restore puts back, which is why `adds_paths` counts as Recipe coverage.
