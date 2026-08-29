# ADR-0053: A Serving Unit is probed, or named in the ratchet

## Status

Accepted, 2026-08-29. Decided in #720; closes the class #716 fixed for headscale alone.

## Decision

**Every App whose Unit Ownership includes a Serving Unit carries at least one Readiness Probe across the roles its Playbook runs.** A Serving Unit is a `.service` installed `WantedBy` a boot target rather than pulled by a timer, classified from the `[Install]` section of the unit's own scanned file; a unit the scan holds no file for is hand-classified in a const checked to stay underivable. `tests/probe_after_restart.rs` enforces presence beside the ordering fence it completes.

**The Meta is the domain authority, not the file scan and not the restart edges.** Three candidates could answer "which Apps deploy a unit":

- `units:` in the Playbook Meta (ADR-0042) — declared intent, already fenced bidirectionally against the scan, and the only inventory that holds the units no task reveals (syncthing's fileless template instance, navidrome's and icecast2's packaged units).
- The unit-file scan — observed behavior, but a role that installs a packaged unit without templating it contributes nothing, so a scan-computed domain reopens the #716 class for exactly the next such role.
- The declared restart edges (`install_notifies_restart`'s tables) — they exist only where an install names a version, so an App outside that model would be outside this one too.

The Meta wins because the other two shrink silently, and a fence over a domain that shrinks silently passes vacuously — the failure mode this fence exists to end.

**The fence lands as a ratchet, not a cliff.** Two exception consts with distinct semantics, the `DEFERRED_HANDLERS`/`SELF_APPLIED` treatment:

- Permanent, reason-carrying: caddy alone. The Ingress Gate judges its restart once per play against the whole assembled config (#568); that is its probe by design.
- Debt, expected to reach empty: `NOT_YET_PROBED`, exactly 12 Apps. An entry leaves only by gaining a probe — the fence fails the moment a listed App probes, so the list cannot quietly hold a solved entry — and each names what its future probe would read (a port, a socket, a loopback endpoint), so the const doubles as the backlog's decomposition. Per-App issues are filed when an entry is picked up, not upfront.

## Why

`probe_after_restart.rs` quantifies over _(notify, probe)_ pairs: it asserts probe **ordering** conditional on probe **presence**. An App with zero probes contributes zero pairs and passes vacuously — headscale deployed a control plane nothing read until #716. The pinned have-probe roster catches the scanner losing sight of a probe it once saw; it cannot catch an App that never had one. Nothing in the suite demanded that a deployed service be read at all.

The classifier keys on `[Install]` because it is the fact systemd itself acts on: `systemctl enable` hooks a boot-target `WantedBy`, while a timer-pulled `.service` carries none — the `.timer` activates it. That one directive separates immich's `RemainAfterExit` oneshot (Serving: it holds the compose stack up) from the eight timer jobs beside it, with no per-unit judgment calls. A `WantedBy` target the classifier does not know is a hard stop, not a quiet "not Serving".

## Trade-off

A debt const is vacuous for the Apps it lists — the fence demands nothing of the 12 until their entries leave. Accepted because the alternative is worse in both directions: writing 12 probes now is per-App ansible work that would bury the fence PR, and shipping no fence leaves the next probeless App invisible. With the ratchet, a new Serving App must probe or visibly join the list in review, which is the #716 hole closed; the listed remainder is enumerated, named, and can only shrink.

## Alternatives considered

- **Per-unit presence** — demand a probe per Serving Unit, not per App. Rejected: paperless runs four Serving Units off one deploy; one probe validates the deploy's outcome, and four probes of one granian would be ceremony.
- **Widening `is_probe`** so more existing tasks count (e.g. `systemctl is-active`). Rejected: reading systemd's opinion of a unit is not reading the service, and #644's readout exists because the two diverge. The recognizer is unchanged.
- **Filing the 12 probe issues upfront.** Rejected: an issue per entry duplicates what the const already states, and goes stale the same way; the entry's doc comment is the brief, and the issue is filed by whoever picks it up.
