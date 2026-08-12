# ADR-0021: Memory Budgets are declared in Playbook Meta

## Status

Accepted, 2026-08-12.

## Context

The Host overflowed physical RAM at least once: 1039 MB sat cold in swap on a 3.8 GB box (#482). The proximate cause was `paperless-task-queue` peaking at 1.4 GB during a one-off ~100-document bulk upload — a legitimate workload with no declared ceiling, so the excursion was absorbed by swapping every other App instead of by throttling the one that caused it.

Memory expectations existed but were invisible. Exactly one budget lived in the repo — grimmory's unit template hardcoded `MemoryMax=1200M` / `MemoryHigh=1100M` — discoverable only by reading that template. Every other App had none. The Host's total commitment was discoverable only by SSHing in and running `systemctl status` per service, and the baseline measured in #482 exists nowhere except that issue's body.

This is the same condition three prior decisions fixed for other per-App concerns: `required_keys` pulled config validation into Preflight, the Backup Recipe pulled backup procedure into declarative data, and the `version:` block pulled Version Resolution into Playbook Meta (ADR-0017). A memory budget is the fourth instance of the shape: per-App operational fact, previously implicit in role internals or absent, wanted reviewable in one place.

Two wrinkles distinguish it from the `version:` precedent:

- An App may run several units — paperless runs four — and their footprints differ by an order of magnitude (webserver 359 MB steady, consumer 47 MB). A single per-App number is either too coarse or wrong, so the budget must be keyed by unit.
- Not every role owns its unit file. Navidrome's ships inside the upstream `.deb`, so there is no template to render into; the budget must reach it as a systemd drop-in.

## Decision

- **A Memory Budget is declared in the App's Playbook Meta**, as a `memory:` block keyed by systemd unit name, each entry carrying `high` (`MemoryHigh=`, the throttle-and-reclaim ceiling) and `max` (`MemoryMax=`, the OOM-kill line). Both are required: a `max` without a `high` skips straight from silent growth to kills.
- **Auberge injects budgets at deploy** through the existing `extra_vars` seam, as `<unit>_memory_high` / `<unit>_memory_max` pairs (unit-name hyphens become underscores), exactly as App Versions travel (ADR-0017).
- **Roles that own the unit template reference the vars directly.** Roles whose unit ships with the upstream package (navidrome) render a drop-in `/etc/systemd/system/<unit>.service.d/memory.conf` instead.
- **Budgets are opt-in per unit**, like `backup:` — set from measured baselines (#482 records the first), not invented at role-writing time. An App without a budget is an App nobody has measured yet, and that absence is visible in review.
- **Tests keep meta and templates from drifting**: every declared budget must be rendered by some role template; no template may carry a literal `MemoryHigh=`/`MemoryMax=` (a budget outside the meta is invisible again — grimmory is the worked example); `high` must not exceed `max`; values must be systemd size syntax.

## Consequences

**Positive:**

- The Host's total memory commitment is reviewable in the repo: `grep -A4 'memory:' ansible/playbooks/*.meta.yml` replaces SSH + `systemctl status` per service. Sizing a new App (#481's Icecast/Liquidsoap estimate) happens against declared numbers.
- A future bulk upload throttles `paperless-task-queue` instead of cold-swapping a gigabyte of everyone else. `MemoryHigh` reclaims page cache and throttles first; `MemoryMax` kills only past the hard line.
- Grimmory's hidden budget becomes a declared one, values unchanged.
- cgroup charging means `MemoryHigh` also bounds page-cache attribution — navidrome's 234 MB cgroup vs 82 MB RSS gap is mostly reclaimable cache charged to it, which a budget reclaims under pressure without any config change.

**Negative:**

- Caps convert silent degradation into loud kills. Units keep `Restart=on-failure`, so a workload that immediately re-exceeds `max` can crash-loop until systemd's start limit halts it — acceptable, because the failing unit names itself in `journalctl`, where the swap storm named nobody.
- Budgets rot as workloads change. They are declared expectations, not measurements; re-measure when an oom-kill or throttling shows up, as #482's acceptance list does for this first set.
- The extra-vars payload grows by two pairs per budgeted unit.

## Alternatives considered

- **Per-App systemd slice with an aggregate budget.** systemd-native aggregation (`paperless.slice`), but it hides which unit blew the budget, requires every role to adopt slice wiring, and the four paperless numbers say more in review than one. More machinery for less legibility.
- **Tune the workloads instead of capping them** (`PAPERLESS_TASK_WORKERS`, celery concurrency). The 1.4 GB peak was a one-off migration burst; permanently detuning steady-state OCR to accommodate it optimizes for the wrong case. Caps bound the excursion and leave the steady state alone.
- **Keep limits hardcoded in unit templates** (status quo, grimmory-style). Invisible in exactly the way this ADR exists to fix, and each role grows its own idiom.
- **Role defaults (`<role>_memory_high` in `defaults/main.yml`).** Scattered across 20+ files and unkeyed by unit; the Playbook Meta is where an App's contract with auberge already lives. Same reasoning that put App Versions there rather than in defaults (ADR-0017).
