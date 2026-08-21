# Can Immich run natively (no Docker) in a supportable way? — research for #550

2026-08-21. Relitigates #549's 2026-08-20 decision ("bare-metal is upstream-unsupported; photos are
the most irreplaceable data, wrong place for exotic deployment") against primary sources.

## TL;DR

Native Immich is _possible_ on Debian 13 — every dependency exists — but not _supportable_: upstream
says plainly it isn't supported, and the only maintained install path is a bus-factor-1 script that
rebuilds Immich from source on the production host and once stalled 85 days across 20 upstream
releases including a major. **Do not reframe #550. Keep compose-wrapped-systemd.** #549's rationale
survives contact with the evidence; this file quantifies it.

## Upstream stance

- Install docs: "Immich requires **Docker** with the **Docker Compose plugin**"
  ([docs.immich.app/install/requirements](https://docs.immich.app/install/requirements)).
- Bug reports structurally assume compose: `bug_report.yaml` has a required field "Your
  docker-compose.yml content" ([.github/ISSUE_TEMPLATE/bug_report.yaml](https://github.com/immich-app/immich/blob/main/.github/ISSUE_TEMPLATE/bug_report.yaml)).
- [Discussion #1657](https://github.com/immich-app/immich/discussions/1657) (open since 2022-09-22,
  79 upvotes, 27 threads) — the team has answered the same way for 4 years:
  - alextran1502 (founder), 2024-06-07: "we have to compile some libraries a specific way … if we
    have to support native deployment, we have to find ways to make it work across different Linux
    environments. That is the trade off of time we can use to focus on developing the application."
  - bo0tzz (core team), 2025-01-28, on NixOS/AUR/native packages: "Note that any of these methods
    are not supported by us."
- One officially _documented_ exception, DB only:
  [postgres-standalone](https://docs.immich.app/administration/postgres-standalone) — external
  Postgres ">= 14, < 20" with pgvector ">= 0.7, < 0.9" and VectorChord ">= 0.3, < 2.0", flagged
  "not officially recommended", and VectorChord updates are manual choreography: "ALTER EXTENSION
  vchord UPDATE; REINDEX INDEX face_index; REINDEX INDEX clip_index."

No recent softening; if anything v3.0.0 (2026-07-02) tightened coupling by dropping pgvecto.rs
([release notes](https://github.com/immich-app/immich/releases/tag/v3.0.0): "chore(server)!: drop
pgvecto.rs support").

## arter97/immich-native anatomy

[Repo](https://github.com/arter97/immich-native): 195 stars, 33 forks, **0 git tags** (contributors
API: arter97 137 commits; next-highest 2 — bus factor 1). Open issue
[#44](https://github.com/arter97/immich-native/issues/44) asks him to co-maintain a real Debian
package; unanswered since 2025-11.

The Debian-flavored fork [sylar/immich-native-debian](https://github.com/sylar/immich-native-debian)
is the abandonment failure mode realized, not an escape from it: a fork of arter97's repo created
2025-02-26 and **last pushed 2025-02-27** — one day of activity, dead for 18 months, frozen at a
pre-v2/pre-v3, pre-VectorChord Immich (5 stars, 0 forks, issues disabled, no license; `gh api
repos/sylar/immich-native-debian`, 2026-08-21).

[`install.sh`](https://github.com/arter97/immich-native/blob/master/install.sh) (`REV=v3.1.0`) is a
**source build on the host**, not a package install:

- `git clone` immich at `$REV`, sed-rewrites `/usr/src` → `/var/lib/immich`, then `pnpm` builds
  server + web + plugin-core (`NODE_OPTIONS="--max-old-space-size=4096"` — 4 GB Node heap).
- sharp: built against system libvips only if ≥ **8.18.3** (`VIPS_TARGET_VERSION`), else prebuilt.
- extism-js installed from js-pdk's **`main` branch** raw install.sh — an unpinned moving dep.
- ML: python venv + `uv sync --extra cpu` (uv downloads its own CPython — host Python decoupled).
- Downloads GeoNames dumps; installs `immich.service` + `immich-machine-learning.service`
  (`Requires=redis-server.service postgresql.service`).

Host deps per [README](https://github.com/arter97/immich-native/blob/master/README.md): NodeSource
Node "v24 LTS", PostgreSQL 18, Redis 8.4.0, PGDG `postgresql-NN-pgvector`, VectorChord,
jellyfin-ffmpeg7 ("FFmpeg provided by the distro is typically too old"), ~35 apt packages. README's
own caveats: "tested on Ubuntu 24.04"; "the install script may get broken if you replace the `$REV`
to something more recent"; CUDA/HW transcoding unsupported; "JPEG XL/RAW support may differ official
Immich due to base-image's dependency differences"; HEIF needs sharp built from source against
latest libvips (offered via his **Ubuntu-only PPA**).

### Release lag (upstream release → immich-native "Release" commit)

| Event                                        | Upstream          | immich-native                                 | Lag                                                                   |
| -------------------------------------------- | ----------------- | --------------------------------------------- | --------------------------------------------------------------------- |
| v3.1.0 (current)                             | 2026-07-29 14:20Z | 2026-07-29 14:32Z                             | **12 min**                                                            |
| v3.0.0 major                                 | 2026-07-02        | v3.0.1 on 2026-07-04                          | 2 days                                                                |
| v2.0.0 major                                 | 2025-10-01        | **skipped**; v2.1.0 on 2025-10-16             | 15 days                                                               |
| Summer 2025 stall                            | v1.136.0 … v2.1.0 | v1.135.3 (2025-07-23) → v2.1.0 (2025-10-16)   | **85 days, 20 stable releases skipped** (incl. both v2.0.x)           |
| VectorChord migration (v1.133.0, 2025-05-21) | —                 | skipped 1.133; 1.132.3 → 1.134.0 (2025-05-31) | 10 days; users hand-installed VectorChord per postgres-standalone doc |

Sources: `gh api repos/{immich-app/immich,arter97/immich-native}` releases/commits, 2026-08-21.
During the stall users filed [#35](https://github.com/arter97/immich-native/issues/35),
[#37](https://github.com/arter97/immich-native/issues/37),
[#38](https://github.com/arter97/immich-native/issues/38),
[#39](https://github.com/arter97/immich-native/issues/39) asking for updates. Today's lag is zero —
the risk is variance, not the mean.

## Debian 13 feasibility (all pieces exist)

- **VectorChord**: [tensorchord/VectorChord v1.1.1](https://github.com/tensorchord/VectorChord/releases)
  (2026-02-28) ships `postgresql-{14..18}-vchord` `.deb`s for amd64/arm64 — no compile needed; 1.1.1
  is inside immich's ">= 0.3, < 2.0" window. pgvector comes from PGDG apt.
- **Postgres**: PGDG publishes all supported majors for trixie, so a native install could even run
  PG 16 to match the pod dump exactly. Host already runs Debian's postgres + redis-server for
  paperless (`ansible/roles/paperless/tasks/main.yml:83-101`).
- **Gaps**: trixie ships vips **8.16.1** ([sources.debian.org](https://sources.debian.org/api/src/vips/))
  < the 8.18.3 sharp-from-source floor → **no HEIC thumbnails for iPhone photos** without hand-built
  libvips (his PPA is Ubuntu-only; even that broke —
  [#46](https://github.com/arter97/immich-native/issues/46)). NodeSource allows one `nodejs` major
  per host: actual pins `node_22.x` (ADR-0016), immich-native wants 24 — coupled upgrade cycles.
  extism-js prebuilt needs glibc ≥ 2.39 ([#42](https://github.com/arter97/immich-native/issues/42))
  — trixie's 2.41 is fine; noted as a fragility class. Debian-13 install confirmed working after
  troubleshooting in [#40](https://github.com/arter97/immich-native/issues/40) (closed).

## Upgrade + restore, compared

|                           | Compose (branch 550)                                                                                                                                                                                                                       | Native                                                                                                                                             |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| Who tests the combo       | Upstream CI ships a tested image set per release                                                                                                                                                                                           | Nobody — arter97 tests on Ubuntu 24.04; auberge would be the Debian-13 integration tester                                                          |
| Upgrade mechanics         | Renovate bumps `IMMICH_VERSION` in `immich.meta.yml` (github-releases datasource, already wired); `compose pull`                                                                                                                           | Re-run `install.sh`: `rm -rf $APP` + full source rebuild on the prod host (4 GB heap); script itself may need changes per release (README warning) |
| ADR-0017/Renovate fit     | Clean — exact shape already in the repo                                                                                                                                                                                                    | Broken — immich-native has 0 tags/releases to track; a Renovate bump of the immich version says nothing about whether the script supports it       |
| DB extension updates      | Bundled in `ghcr.io/immich-app/postgres` image tag                                                                                                                                                                                         | Manual `ALTER EXTENSION vchord UPDATE` + two `REINDEX`es per the standalone doc                                                                    |
| Backup recipe             | Nonstandard: cold copy of `/var/lib/immich/{upload,postgres}` with unit stopped, `post_restore_command` chown (`immich.meta.yml`) — container PG is unreachable by the executor's host `pg_dump` (`src/services/backup/executor.rs:35-46`) | Standard ADR-0001 `db:` recipe works (paperless precedent, `paperless.meta.yml:24`) — genuinely nicer                                              |
| Restore-time dependencies | ghcr pulls of repo-pinned image tags                                                                                                                                                                                                       | Source rebuild: GitHub + npm registry + PyPI + GeoNames + NodeSource + extism-js@main (unpinned) must all cooperate during disaster recovery       |

## Risk profile for irreplaceable photo data

| Failure mode                                           | Compose                                                                                                                                 | Native                                                                           |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Untested app/runtime combo silently corrupts or breaks | Low — upstream-tested image set                                                                                                         | Real — README: script "may get broken" on newer `$REV`; JPEG XL/RAW "may differ" |
| Third-party abandonment stalls security updates        | No third party in the path                                                                                                              | Demonstrated: 85-day stall over 20 releases; bus factor 1                        |
| Extension/ABI drift (PG major, vchord, glibc, libvips) | Absorbed by image tags                                                                                                                  | Operator-managed across 4 independent upgrade streams                            |
| Disaster recovery blocked by an upstream outage/change | One registry                                                                                                                            | ≥6 online sources incl. one unpinned branch                                      |
| Docker daemon as ops/attack surface, ~200-300 MB RSS   | Accepted (contra `meta/adr.md` "No Docker")                                                                                             | Absent                                                                           |
| Docker iptables bypassing ufw/fail2ban                 | **Neutralized** — every published port bound `127.0.0.1` (`ansible/roles/immich/files/docker-compose.yml`); live check in #549 ops list | N/A                                                                              |
| HEIC (iPhone) thumbnails missing                       | Works (upstream base image compiles libvips/libheif)                                                                                    | Broken on trixie without hand-built libvips ≥ 8.18.3                             |

## What survives from branch 550 either way

Caddy vhost + `dns_record` (`subdomain: photos`), the `version:` block, localhost-only exposure,
systemd as the control surface, and the stop-unit-then-rsync recipe shape. Only the `docker` role,
the vendored compose file, and the cold-copy DB path are compose-specific.

## Recommendation

**Keep #550 as scoped: compose wrapped in systemd. Do not reframe around native.** #549's two
clauses both check out against primary sources: "upstream-unsupported" is literal ("not supported by
us" — bo0tzz; a bug template that cannot be filed without a compose file), and "wrong place for
exotic deployment" is now quantified (bus factor 1, 85-day stall spanning the v2.0.0 major, source
rebuild on the prod host per upgrade, HEIC broken on trixie). Record this file as evidence in #551's
ADR-0025, with revisit triggers: upstream ships a `.deb`/official native path, or immich-native
gains tags + CI + a second maintainer (the #44 Debian-packaging effort maturing).

**Strongest argument against this verdict**: it trades the repo's identity for upstream's comfort.
Auberge is bare-metal _by decision_ (`meta/adr.md` "No Docker"; ADR-0016 chose npm+NodeSource over
docker for actual); the host already runs the substrate a native Immich needs (Debian postgres,
redis, NodeSource); VectorChord ships trixie-ready `.deb`s for PG 14–18; the DB half is officially
documented; the standard `db:` backup recipe would work natively while compose forces the repo's
first nonstandard cold-copy recipe; and immich-native's lag _today_ is 12 minutes. If arter97 keeps
pace — as he has since Oct 2025 — native works, and NixOS proves the packaging is tractable.
The rebuttal is that "if the maintainer keeps pace" is precisely the bet: summer 2025 shows the
downside realized, and the photo library is the one dataset where auberge should not be the
integration test.
