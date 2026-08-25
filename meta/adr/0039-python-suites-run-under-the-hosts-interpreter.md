# ADR-0039: A Python suite runs under the Host's interpreter, from the role's dependency list

## Status

Accepted, 2026-08-25. Closes #643.

## Decision

`mise.toml` declares `test-python`, which `test`'s `depends = "test-*"` picks up. It runs pytest under `uv run`, pinned to `--python 3.13` — the Host's interpreter minor — and provisions `pytest` plus the exact packages the baikal role installs into the **Busy Feed** venv, `--with` each and unpinned, mirroring the role's `ansible.builtin.pip` list.

`.github/workflows/master.yml`'s `check` job runs that command byte-for-byte, ungated.

`tests/python_test_pipeline.rs` fails the build when any of three stop holding: a `test_*.py` exists in a directory the task does not collect from; a package the role installs into the venv is not provisioned by the task; the workflow does not contain the command the task declares.

## Why

**61 tests ran nowhere.** `mise r test` resolved `test-*` to `test-rust` and `test-shell`; neither the workflows nor `hk.pkl` mentioned pytest. The only thing that ever invoked the two baikal suites was a hand-typed `uv run`. Both scripts they cover have shipped silent data-loss bugs — #616 dropped blob-stored birthdays, #637 dropped the 10 whose `BDAY` omits the year while printing `Synced 66 birthdays`, #484 shifted floating iCloud times by the Berlin offset — and every one of them exited 0. PR #638 grew one suite from 8 tests to 31; its entire proof of correctness rested on a runner nothing called.

CodeQL's `analyze (python)` job (ADR-0011) runs on every PR and its green check reads like Python coverage. It is static analysis. It executes no test.

**The interpreter is pinned because the Host's is.** auberge runs Debian 13.6 and Python 3.13.5; `/opt/baikal/busy-venv` is the same 3.13.5. Unpinned, uv resolves whatever it finds, and the three places this suite runs disagree: the development machine this was written on reports `cpython-3.14.7` as its system interpreter, an `ubuntu-24.04` runner ships 3.12, and the Host executes 3.13.5. That is three interpreters, none of them guaranteed to be the Host's, for a suite whose entire job is to speak for what the Host does. `--python 3.13` collapses it to one. The minor is pinned and the patch is not, matching what Debian's own updates are free to move.

**The packages are unpinned because the role's are.** The role installs `icalendar`, `recurring-ical-events` and `caldav` with no version constraint, so the Host resolves the latest at deploy time. Pinning the test side would make the suite green against bytes the Host does not run — the failure this ADR exists to close, one level up. The `--with` list is the same list, and the guard keeps it the same list.

**uv, because uv is already here.** `hermes_uv_version` and `tgtg_uv_version` pin uv 0.12.5 as **Tool Versions** (ADR-0017), so this adds no dependency in kind, only a `[tools]` entry. `--with` builds an ephemeral environment with no lockfile and no venv to maintain. Measured, against the 61 tests' own 0.5s: 0.9s warm; 6.8s with a cold package cache; 16.3s with a cold cache _and_ no 3.13 interpreter present, which is a runner's state on every run — uv's `python-preference` defaults to `managed`, so it downloads its own CPython rather than adopting a system one.

**The CI copy is asserted, not trusted.** `jdx/mise-action`'s `mise_toml:` input writes its content to `mise.toml` in the workspace — `##[group]Writing mise.toml` in the job log — replacing the repo's. The `check` job therefore cannot call a task this repo declares; it has to repeat the command. Left to convention that repetition drifts, and it already has: `test-shell` lists five scripts and the workflow runs three, so `tests/immich-b2-prune.test.sh` and `tests/immich-backup.test.sh` execute in no pipeline at all today — #643 one layer down, undetected for the same reason.

**Ungated, in `check`.** The suites live under `ansible/`, so `_test`'s changed-files gate would have covered them, but `_test` is the Rust job — a Rust toolchain and nextest. `check` finishes in ~50s against `_test`'s ~2min, so on any PR that also touches Rust or ansible the ~16s is absorbed by a job that is not the critical path; only a docs-only PR, where `_test` is skipped, pays it in wall-clock.

## Considered alternatives

- **A `pyproject.toml` or lockfile.** Renovate could then track the three packages, and that is the point against it: a pinned test environment drifts from an unpinned deployed one, so the role would have to pin too. Whether the **Busy Feed** venv should be pinned is a deploy decision (ADR-0017's regime question), not a test-pipeline one. The task can read a manifest the day the role installs from one.
- **Run them in `_test`, on its existing gate.** The gate is sound — the suites are under `ansible/**` — but it buys nothing: `check` is not the critical path, so gating saves no wall-clock and costs an unconditional signal.
- **A third job.** A second mise install and a new status check name, for ~16s of work.
- **Cache `~/.cache/uv` and the managed interpreter with `actions/cache`.** Would take the ~16s down to roughly the 0.9s warm figure, and is declined for now: the cost lands on a job that is not the critical path, so it buys wall-clock on doc-only PRs alone, against a cache key to maintain and ~150MB to restore and save on every run. Worth revisiting if `check` ever becomes the long pole.
- **pipx, or a checked-in venv.** pipx installs applications, not ad-hoc environments. A venv is state to create, refresh and gitignore; `--with` is none of that.
- **Leave them to `uv run` by hand.** The state #643 describes.

## Consequences

**Positive:**

- `mise r test` fans out to 791 Rust tests, the shell harnesses and 61 Python tests; a broken assertion in either suite returns 1 (measured, not assumed).
- The same 61 run on every PR, on a job that is not the critical path.
- A `test_*.py` landing anywhere in the repo fails the build until the task collects it — the recurrence #643 warns about is fenced, not documented.
- A package added to the **Busy Feed** venv fails the build until the task provisions it, so a new import cannot be green in CI and absent on the Host.
- The workflow's copy of the command cannot diverge from the task's.

**Negative / accepted:**

- Every PR pays the uv resolve and the interpreter download, including doc-only ones: ~16s measured, uncached by choice.
- Renovate tracks none of the three packages. Same posture as the role, deliberately.
- `uv` joins `[tools]` at `latest` while the roles pin 0.12.5. A dev tool and a deployed artifact are held to different regimes; ADR-0017 governs only the latter.
- The dependency guard is baikal-specific — the only role today with both a Python suite and a venv. A second one extends the test, which is the point at which someone decides whether the two venvs share a list.
- No Python linter or formatter is configured anywhere, and this adds none (#643, out of scope).

## References

- Issue #643 — the gap, and the acceptance criteria this meets.
- Issues #616, #637, #484 — the three exit-0 bugs these suites cover.
- PR #638 — the suite that tripled in size with no runner behind it.
- ADR-0010 — the **Busy Feed**, whose venv the `--with` list mirrors.
- ADR-0011 — the CodeQL advanced setup whose `analyze (python)` check reads like coverage.
- ADR-0017 — the Tool Version regime uv is pinned under in the hermes and tgtg roles.
