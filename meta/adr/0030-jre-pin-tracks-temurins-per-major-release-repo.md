# ADR-0030: The JRE pin tracks Temurin's per-major release repo

## Status

Accepted, 2026-08-23. **Applies ADR-0017 and ADR-0027 to grimmory's JRE** — a Tool Version on the versioned-dest regime — and records why the pin's Renovate wiring is not the obvious one.

## Decision

Grimmory's JRE is pinned to a full Temurin version — `25.0.4.1+1`, build suffix included — and the dest carries it (`/opt/java/temurin-25.0.4.1+1-jre`), so every bump is a path that cannot already exist and the `stat` guard is honest instead of a freeze. The download notifies `Restart grimmory` and superseded `temurin-*-jre` trees are reaped, mirroring the jar.

Renovate tracks the pin as `datasource=github-releases depName=adoptium/temurin25-binaries` with `extractVersion=^jdk-(?<version>[\d.+]+)$`, plus a `renovate.json` packageRule setting a `regex:` versioning that ranks the 4th version digit (`build`) and the `+` suffix (`revision`).

Majors are LTS-only, structurally: Adoptium keeps one GitHub repo per major, so the pin is incapable of offering a cross-major bump. Moving to the next LTS is a deliberate edit of the depName, the URL's repo path, the archive's `OpenJDK25U` prefix, and the version — adjacent lines in `defaults/main.yml` plus the packageRule's `matchDepNames`.

## Why

`grimmory_java_major: "25"` named thousands of builds, `binary/latest/25/ga` let Adoptium decide which one landed, and the major-only dest froze whichever arrived first (#607). A major bump would have self-healed through the unit re-render; the 25.x → 25.y security updates had no path at all, on the JVM under a Public App.

### Why not the `java-version` datasource

It is the purpose-built datasource and it does not fit, for reasons verified against Renovate source and live Adoptium probes (2026-08-22):

- It emits Adoptium's _semver_ field — `25.0.4+101.0.LTS` for the release Adoptium names `jdk-25.0.4.1+1` (respins fold into build metadata as `patch*100+build`). That string cannot be templated into any download URL; resolving it needs a `/v3/assets/version/…?semver=true` JSON call at deploy time, a two-step no role in this repo has.
- Renovate's default `semver-coerced` versioning discards everything past the patch, so `25.0.4+7` → `25.0.4+101.0.LTS` compares equal — the respins, which are the security releases, would never be offered. The datasource's defect and the default versioning's defect compound.

`github-releases` on the per-major repo produces release-name versions (`25.0.4.1+1`) that template directly into the asset URL — the same URL Adoptium's own API 307-redirects to — and matches how every other Tool Version in this repo is annotated.

### What the regex versioning is blind to, by design

- Same-`x.y.z` re-releases that only raise the `+build` compare equal. Temurin GA never ships those; respins bump a 4th digit instead (`jdk-25.0.4+7` → `jdk-25.0.4.1+1`). EA is where `+build` increments, and EA is excluded twice over: prerelease-flagged releases are dropped by `ignoreUnstable`, and `-ea-beta` tags fail the `extractVersion` anchor.
- The initial GA of a major (`25+36`) has no minor.patch and does not match. Irrelevant under LTS-only: a new major is a manual edit, never a Renovate offer.

The versioning lives in a packageRule rather than inline in the `# renovate:` annotation only because the pattern pushes the line past yaml line-length.

## Considered alternatives

- **Checksum the download** against Temurin's `.sha256.txt` sidecar. Declined: the jar itself — the actual App — downloads with no checksum, as does every other Tool Version artifact. Verifying only the JRE is theater; a checksum policy would be repo-wide and its own decision.
- **Keep `binary/latest` and force the download.** The retired Latest-at-Deploy regime (ADR-0017): the Host's bytes stop being decided by the repo.
- **Widen `tests/install_notifies_restart.rs` to fence this.** That fence models App Versions; ADR-0028's declared regime covers App installers run by units the role does not template. A Tool-Version artifact→restart edge (this JRE, blocky's lego) is a third category, inventoried on #605 rather than half-fenced here.

## Consequences

**Positive:**

- Temurin security respins reach the Host as ordinary Renovate PRs; two Hosts provisioned apart converge on identical JREs from identical repo state.
- The legacy unversioned `temurin-25-jre` tree is reaped by the same task that reaps superseded pins.
- The interrupted-deploy rerun converges: the unarchive notifies the restart itself, so a run that dies between unit re-render and handler flush is repaired by the next one.

**Negative:**

- Every bump re-downloads ~180 MB and bounces grimmory once — including the first deploy of this change, a pure path migration of the same bytes.
- Two intermediate vars (`grimmory_java_release`, `grimmory_java_archive`) exist only to keep the URL under yaml line-length, and the pin's Renovate config spans two files.
- The asset-name template (`OpenJDK25U-jre_x64_linux_hotspot_<version with '+'→'_'>.tar.gz`) is upstream convention, not contract. A rename 404s the deploy — loud, at deploy time, on a reviewed bump.

## References

- Issue #607 — the freeze this retires; its draft annotation is the `java-version` alternative rejected above.
- ADR-0017 — Tool Version; ADR-0027 — versioned dest as the preferred install regime (#597 did the jar).
- ADR-0028 / #605 — why this edge is declared out of the restart fence's model.
- Renovate `lib/modules/versioning/regex` and `lib/modules/datasource/java-version` sources; live probes of `api.adoptium.net` and `adoptium/temurin25-binaries` release assets, 2026-08-22.
