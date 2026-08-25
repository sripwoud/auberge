# ADR-0037: Liquidsoap installs from the Debian archive

## Status

Accepted, 2026-08-25. **Reverses the install path merged in #490** and restores the shape ADR-0020 recorded and the fleet docs never stopped describing: Icecast2 and Liquidsoap, both from apt. Radio's `radio/liquidsoap` Tool Version (ADR-0017) is retired with it.

## Decision

Liquidsoap installs from `trixie/main`, in the same apt task as Icecast2. No version pin, no upstream download, no `# renovate:` annotation — apt chooses the bytes, exactly as it does for Icecast2 and Caddy.

The pinned-release scaffolding dies with the pin: the `radio_liquidsoap_version` / `_deb` / `_deb_url` defaults, the `get_url` download, and the `package_facts` + `set_fact` version guard whose only job was comparing the installed version against the pin. Idempotence is apt's (`state: present`); the role no longer implements its own.

## Why

The savonet trixie release asset is uninstallable on a stock Debian 13, and the failure is an **epoch** wall, not a version gap. The `.deb` sitting in `/tmp` on auberge declares all seven ffmpeg libraries at deb-multimedia's epoch — `libavcodec61 (>= 10:7.1.3)` and siblings — while trixie's only candidate is `7:7.1.5-0+deb13u1`. Epoch dominates dpkg version comparison: `7:7.1.5` never satisfies `>= 10:7.1.3`, however far ahead 7.1.5 is upstream. Savonet builds against deb-multimedia's ffmpeg, so every trixie asset carries the same floor — no version bump escapes it, which is what disqualifies the whole install path rather than one release.

The first deploy to the new host died at that wall partway through the play, leaving `icecast2` installed but unconfigured and never started (#634). The removed version guard is its own small indictment: its `regex_replace('^\d+:', '')` stripped the epoch before comparing — the scaffolding discarded exactly the field its install path would die on.

Three measurements sharpened the revert into a restore:

**The pinned path never ran anywhere.** Vieille's dpkg log shows Debian's `2.3.2-2+b1` installed 2026-08-12 09:51 and removed 11:30 the same day — the #481 measurement window — and no liquidsoap install since. #490 merged that evening; its download-and-install path's first-ever execution was the 2026-08-25 failure on auberge. The revert gives up a benefit that was never realised.

**#490's other half was never run either — and does not run.** The `pcm_s16` frame request added alongside the pin ("works on the deployed 2.3.2: pcm_s16 shipped in 2.3.0") postdates that same 11:30 removal, and on 2.3.2 it is fatal: the encoder rejects it at instantiation with `Unknown encoder parameter: pcm_s16`, crash-looping the unit into its start limit. The parameter is 2.4 surface, and it goes with the pin. The failure's shape is worth recording: encoder parameters are validated at instantiation, not at type-check, so `liquidsoap --check` passes the script that crashes — and instantiation happens per-station, so the zero-station idle path starts clean and the crash waits for the first `.m3u` to land.

**The Memory Budgets were sized on apt's 2.3.2.** The 448M → 164M `OCAMLRUNPARAM=o=40` measurement behind the 320M/384M budgets (ADR-0021, #481) was taken in that same Aug 12 window, on `2.3.2-2+b1`, before `pcm_s16` existed in the script. Dropping both returns to the measured configuration exactly.

## Considered alternatives

- **Add deb-multimedia.org as an apt source.** Makes the asset installable, and rejected for what else it does: deb-multimedia's epoch 10 outranks Debian's epoch 7 on the whole ffmpeg family, box-wide and permanently — epochs are sticky, so once installed, Debian's own ffmpeg can never win a comparison again, on this release or any future one. That swaps the media stack under every other consumer on the host and moves its security response from `trixie-security` to a third-party repo, to serve one encoder.
- **Bump the pin to a newer savonet release.** The reflex Renovate automates, and a no-op here: every savonet trixie asset declares the same epoch-10 floor. This is the second structural hazard of this asset family — #490 already documented the first (the `ocaml4.14.2-2` filename suffix that 404s stale-suffix downloads), and MEMORY's #607 lesson (respins invisible to the `github-releases` datasource) is the third. An install path needing this many footnotes is describing its own removal.
- **Force the install past dpkg** (`--force-depends` or equivalent). Lying to the package manager about the ABI dependencies of a media encoder; the first ffmpeg point release turns the lie into a runtime crash.
- **Build from source / opam on the host.** An OCaml toolchain and a build farm's worth of dev packages on a small VPS, to reproduce bytes the archive already ships.
- **Savonet's container image.** ADR-0025 grants container exceptions only when no native install delivers the App — this very apt package is the native install, so radio cannot clear test 1. AzuraCast was already rejected on the same ADR.

## Consequences

**Positive:**

- `auberge deploy radio` completes on a stock trixie; nothing is fetched from GitHub at deploy time.
- Security updates for liquidsoap arrive through `trixie-security` unattended, like every other apt package on the host.
- The 404-trap (filename suffix) and respin hazards of the release asset are gone with the asset.
- Reintroducing the pin fails the build: `radio_liquidsoap_version` is out of the `TOOL_VERSIONS` allowlist, and the allowlist is asserted bidirectionally (`tests/version_annotations.rs`).
- ADR-0020 and `docs/applications/apps/radio.md` — which #490 never updated — are accurate again without an edit. `CONTEXT.md`'s **Installed Version** entry and ADR-0027's Artifact-read examples drop radio, the one place the glossary tracked the deleted guard.

**Negative / accepted:**

- Trixie ships 2.3.2, one minor behind upstream; the GC allocation optimizations savonet pointed to in savonet/liquidsoap#748 and the `pcm_s16` frame memory saving are both given up. Accepted because the budgets were sized on 2.3.2 without either and hold with ~2x headroom, and because neither figure was ever measured here — the pin never installed and the parameter never ran.
- Renovate no longer tracks liquidsoap at all. Same posture as Icecast2 and Caddy: apt chooses the bytes, and a `--check-upstream` drift report is traded for the archive's own update cadence.
- If 2.4.x's memory behaviour is ever genuinely needed, the epoch wall stands until Debian ships a 2.4 (forky) or savonet builds against Debian's ffmpeg. Reopening that door means revisiting this ADR, not re-adding the pin.

## References

- Issue #634 — the failed deploy, the epoch analysis, and the residue on auberge.
- PR #490 — the reversed decision, and the filename-suffix hazard it documented itself.
- ADR-0017 — the Tool Version regime this pin exits.
- ADR-0020 — the Radio's recorded shape ("both from apt") this restores.
- ADR-0021 — the Memory Budgets whose sizing measurement this returns to.
- ADR-0025 — why a container is not the escape hatch.
- ADR-0027 — amended: radio's `package_facts` read retired with the pin, so radio leaves its Artifact-read examples.
- savonet/liquidsoap#748 — the upstream memory thread that motivated #490.
