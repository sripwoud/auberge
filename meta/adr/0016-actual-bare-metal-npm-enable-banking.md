# ADR-0016: Actual deploys bare-metal from npm; bank sync uses Enable Banking

## Status

Accepted, 2026-08-03.

## Context

Actual Budget's sync server is distributed two ways: a Docker image and the npm package `@actual-app/sync-server`. Auberge's native-systemd default (`meta/adr.md` §"Native systemd by default", ADR-0025) leaves npm as the only upstream channel — there is no standalone binary or `.deb` to mirror the navidrome/gokapi install patterns.

Two sub-decisions follow from picking npm:

1. **Node runtime.** sync-server 26.8.0 declares `engines: node >=22`. Debian trixie's apt ships nodejs 20.19 — installing `nodejs`/`npm` from Debian (the `claude_code` role's approach) cannot run the package.
2. **Version resolution.** The repo has two precedents: navidrome re-resolves the GitHub _latest_ release on every deploy; gokapi pins an exact version plus a sha256 in role defaults.

Separately, bank sync needs a PSD2 aggregator. Actual supports GoCardless, SimpleFIN, Pluggy, and Enable Banking — but GoCardless Bank Account Data stopped accepting new accounts in July 2025 (Actual's own docs: "GoCardless has stopped accepting accounts for this service"), SimpleFIN is US-only, and Pluggy is Brazil-only.

## Decision

- Install `@actual-app/sync-server` at an exact pinned version (role default `actual_version`) into `/opt/actual` via `community.general.npm` with `path`, not `global`.
- Node comes from the NodeSource `node_22.x` apt repository. The major is pinned by the repo URL; minor/patch security updates flow through normal `apt upgrade`.
- Bank sync uses Enable Banking. Its application credentials are entered once in Actual's web UI and persist in the sync server's `account.sqlite` — inside the Backup Recipe, outside the Key Registry. Auberge ships no bank credentials.

## Consequences

**Positive:**

- Reproducible deploys: the same role revision installs the same bytes. npm's registry integrity (sha512 + provenance attestation on this package) plays the role gokapi's sha256 pin plays for GitHub archives.
- Upgrades are deliberate one-line bumps of `actual_version` — wanted for a finance app whose CalVer monthlies can break between releases, and consistent with how a version bump (not a re-deploy) is what triggers reinstall.
- Enable Banking is the only self-serve option still open to new EU users, is natively supported by sync-server (`app-enablebanking` module), and covers C24 (AIS + PIS integrations since Enable Banking's October 2024 changelog).

**Negative:**

- A NodeSource outage or repo removal blocks first deploys (not restarts). Accepted: same class of third-party-repo risk as Caddy's cloudsmith repo.
- Node 22 lands on the host for one app. Accepted: ~60 MB, no daemon.
- Actual flags Enable Banking support as experimental and reserves the right to remove it. Mitigated: bank sync is an enrichment, not the budget's source of truth — manual import always works.
- No sha256 in role defaults (npm has no stable artifact URL to checksum). Registry-level integrity + provenance is the substitute.

## Alternatives considered

- **Debian's nodejs + npm packages** (claude_code precedent). Rejected: trixie ships 20.19, below the `engines` floor; apt `npm` would also drag in Debian's nodejs alongside NodeSource's.
- **GitHub-latest-release resolution** (navidrome precedent) via `dist-tags/latest`. Rejected: every deploy could silently jump a CalVer monthly; a budget server should not pick up breaking releases as a side effect of an unrelated deploy.
- **`npm install --global`.** Rejected: scatters the app into `/usr/lib/node_modules`, couples it to the Node package's own upgrade cycle, and breaks the `/opt/<app>` install-dir convention every other role follows.
- **GoCardless for bank sync.** Rejected: closed to new signups since July 2025; no account can be created for this deployment.
- **SimpleFIN / Pluggy.** Rejected: US-only / Brazil-only; the operator's bank (C24) is German.
