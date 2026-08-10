# ADR-0017: App Versions are declared in Playbook Meta; Tool Versions stay in role defaults

## Status

Accepted, 2026-08-10.

## Context

Auberge deploys 16 Apps and has no automated way to learn that any of them has a new upstream release. The gap is not theoretical: `headscale_version` sat at `0.25.1` while upstream shipped `0.29.3` — four minor versions, spanning a policy-v2 rewrite and database migrations, on the VPN control plane — and nothing in the repo or CI noticed.

Three incompatible **Version Resolution** regimes had accumulated (see `CONTEXT.md`), and ADR-0016 already recorded the split without naming it: "the repo has two precedents: navidrome re-resolves the GitHub _latest_ release on every deploy; gokapi pins an exact version plus a sha256 in role defaults."

- **Pinned** — 15 version literals across 12 roles in `defaults/main.yml`.
- **Floating** — freshrss tracks the `edge` branch.
- **Latest-at-Deploy** — navidrome, baikal, and blocky query `api.github.com/.../releases/latest` at run time. The repo records nothing. Baikal additionally guards install with `when: not baikal_installed.stat.exists`, so it is latest at _first_ install and frozen thereafter, at a version stored nowhere.

Only the Pinned regime is legible to a dependency-update bot. Adopting one therefore forces a prior question: where does a version belong?

Mapping versions onto Playbook Meta looked misaligned at first — `caddy` and `tgtg` carry version variables but have no meta file. The misalignment turned out to be a conflation of two different things:

- An **App Version** is the identity of a deployed App. Exactly one per App. It is what an operator, a CVE advisory, and a restore procedure all refer to.
- A **Tool Version** is a build or runtime input the role happens to need — `uv`, `lego`, Caddy's `l4` and `cloudflare` plugins. Nobody asks which `lego` a homelab runs.

Under that split every apparent exception resolves. Caddy has no App Version at all (Caddy itself comes from apt/cloudsmith); its two pins are plugin Tool Versions, so no `caddy.meta.yml` is needed. `tgtg` has one of each — and already lacked a meta file, so it has no `required_keys` validation and no Backup Recipe regardless of this decision. Blocky is Latest-at-Deploy for itself and Pinned for lego, which the split describes exactly.

Two facts made relocation cheap. `run_playbook` already accepts `extra_vars: Option<&[(&str, &str)]>` — a seam that is `None` at all four call sites — and `ansible_runner.rs` already serializes validated `Config` to a temp file passed as `--extra-vars @file`. Roles in this repo are therefore _already_ not standalone: `domain`, `admin_user_name`, and every secret arrive by injection. The Ansible convention that a role must be self-contained was surrendered long before this ADR.

## Decision

- **App Version is declared in the App's Playbook Meta**, as a typed `version:` block carrying the value plus its upstream coordinates (`datasource`, `depName`, and `extractVersion` where the tag format differs from the value). `PlaybookMeta` gains the field; serde validates it; a CI test asserts every App declares one, mirroring `test_all_committed_playbooks_have_meta_files`.
- **Auberge injects App Versions at deploy** through the existing `extra_vars` seam. Role tasks continue to reference `{{ <role>_version }}` unchanged.
- **Tool Versions stay in `defaults/main.yml`**, each carrying a `# renovate: datasource=… depName=…` annotation. A CI test asserts every remaining `_version:` in defaults carries one.
- **Renovate manages both files.** Bumps are committed as `build(<app>): bump to X`, which matches `release_commits` in `release-plz.toml` and therefore cuts an auberge release — deliberate, so a bump becomes reachable by operators rather than accumulating unreleased on master.
- **Co-pinned checksums remain literals in the repo.** A CI job on the Renovate PR downloads the asset, computes the sha256, and commits it onto the same branch; a bump whose checksum job fails does not merge.
- **Grimmory is excluded from Renovate** but still declares an App Version. It is auberge's own build output (`build-grimmory.yml`, tagged `grimmory/vX.Y.Z` in this repo), not a dependency. Renovate scope and schema scope are different questions.
- **Floating and Latest-at-Deploy converge on Pinned.** freshrss, navidrome, baikal, and blocky are converted to declared App Versions, after which `CONTEXT.md` documents one regime.

## Consequences

**Positive:**

- One answer to "what version does auberge install?", readable by a program. `auberge versions` becomes a thin reader over data that already exists rather than a new subsystem.
- Version Resolution stops being implicit in task logic. This completes an arc the repo has followed three times: `required_keys` pulled config validation into `Preflight`, the Backup Recipe pulled backup procedure into declarative data, `tailnet_only` pulled DNS placement into DNS Publication.
- A drifting App becomes visible in review rather than after an incident. The headscale gap is the worked example.
- Deploys stay reproducible: the same role revision installs the same bytes, extended to the four roles where that was previously untrue.
- Literal checksums keep their real property — immutability after merge, which detects an upstream re-tagging or swapping a release asset. Fetch-at-deploy verification cannot detect that, since an attacker able to replace a binary can replace the checksum file alongside it.

**Negative:**

- A version now lives outside `defaults/main.yml`, contradicting Ansible convention. Mitigated: roles were already injection-dependent, and this ADR is the record for the reader who wonders why.
- ~2 auberge releases per month of pure dependency churn. Accepted: the release race window that once made frequent releases costly is closed on both sides (`git_release_draft = true` plus the `publish-release` job; the mise cache bug fixed upstream).
- Renovate discovers upstream releases but cannot deploy them. The chain — merge bump → merge release PR → operator upgrades auberge → `auberge deploy` — stays manual. "We have Renovate" must not be read as "we are current."
- `tgtg` needs a meta file it never had, widening PR2 slightly.
- Small projects (gokapi, bichon) may never file a GHSA, so vulnerability-flagged updates will look routine for them.

## Alternatives considered

- **Annotation comments in `defaults/main.yml` only, no schema change.** Ships in days and is self-maintaining. Rejected as the end state because it leaves Version Resolution undeclared — the same condition that let three regimes diverge unnoticed — though it is adopted as the first increment precisely because headscale is stale now.
- **One `customManagers` entry per variable in `renovate.json`.** Rejected: 15 central blocks that must be edited whenever a role is added or renamed. It rots into the exact failure it exists to prevent, and a forgotten entry is silent.
- **Drop the literal checksums; fetch upstream checksums at deploy**, as blocky and colporteur already do. Rejected: gokapi publishes no checksum file at all — its `51dbf724…` was computed by hand — so gokapi would end up with no verification, and every role would lose re-tag detection.
- **Self-hosted Renovate with `postUpgradeTasks`** to recompute checksums in-process. Rejected: `postUpgradeTasks` is unavailable on the hosted app, so choosing it would smuggle a hosting decision into a checksum decision. A CI job reaches the same result without that coupling.
- **Migrating to NixOS (#179)**, which would make version pinning intrinsic rather than a convention. Out of scope here and not blocked by this decision: the App Version declarations are prior art a Nix migration would consume, not work it would discard.
