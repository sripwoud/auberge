# ADR-0025: Containers are per-app exceptions, granted only when the App is required and upstream supports nothing else

## Status

Accepted, 2026-08-21.

## Context

`meta/adr.md` carried a foundational "No Docker" section: systemd services instead of containers, argued on memory grounds (a 2 GB VPS cannot spare 200-300 MB for a daemon). Written as a prohibition, it did real work — it is why ADR-0020 rejected AzuraCast and why ADR-0016 took Actual's npm sync-server over its Docker image.

Immich (#549, #550) is the first App where the prohibition produced the wrong answer. Upstream requires Docker with the Compose plugin and disclaims every other path: the team has answered the same native-install request the same way for four years, core-team answers call NixOS/AUR/native packages "not supported by us", and the bug report template makes `docker-compose.yml` a required field. The only maintained native installer is a bus-factor-1 script that rebuilds Immich from source on the production host and once stalled 85 days across 20 upstream releases including a major; its one Debian fork was pushed on a single day in 2025 and has been dead since. Primary sources and revisit triggers are in `meta/research/immich-native.md`. Photos are the most irreplaceable data in the fleet — the wrong place for an unsupported deployment.

A prohibition with one violation in it is not a rule, it is a habit. Either the prohibition holds and Immich runs on an abandoned source-build script, or the stance becomes a default with a written grant procedure. Left implicit, every future App relitigates the whole question from scratch.

## Decision

**Native systemd is the default. A container is a per-app exception, argued in writing before the code lands.**

An exception is granted only when **both** tests pass:

1. **The App is required.** No native-installable alternative delivers what is needed. AzuraCast is Docker-only, so it would clear test 2 — it was rejected on this one, because a native design (an m3u file plus liquidsoap, ADR-0020) answered the need.
2. **Upstream supports nothing else.** Evidenced by primary sources — upstream docs, maintainer statements, issue templates, the health of any third-party native path — recorded in a `meta/research/` file with revisit triggers, before the code PR. Actual Budget fails this: `@actual-app/sync-server` is a supported upstream channel (ADR-0016).

Convenience, image availability, and "it is easier to vendor a compose file" are not reasons. Neither test is satisfied by an unmaintained community packaging effort.

When granted, the exception takes a fixed shape:

- **One systemd unit is the whole control surface.** The compose stack is wrapped in `Type=oneshot` + `RemainAfterExit=yes`, `ExecStart=docker compose up -d --wait` / `ExecStop=docker compose down`. `systemctl stop <app>` really stops the containers, so the App presents the same control interface as every other App to `auberge deploy`, `systemctl` and Cockpit. `Requires=docker.service`.
- **Published ports bind to `127.0.0.1`; Caddy proxies.** Docker rewrites iptables and a `0.0.0.0` publish would be reachable past ufw and fail2ban, whose rules would still read as intact. Loopback-only publishing keeps their view of the host authoritative.
- **Images are pinned to exact versions.** No `:latest`, no floating `:-release` fallback. The App Version is declared in Playbook Meta (ADR-0017) so Renovate proposes and the operator disposes; a restore requires the version the dump came from.
- **The compose file is vendored in the role's `files/`, never fetched at deploy.** What runs on the host is reviewable in the repo diff, and upstream editing its release asset cannot change a deployed stack.

**A granted exception also suspends the Backup Recipe (ADR-0001).** A container's Postgres is unreachable from the executor's host `pg_dump`, and `backup create` re-transfers the full dataset through the operator's machine on every run with the stack stopped — nightly, for a multi-GiB photo library. The plan of record through #550 was a cold data-dir copy in the recipe shape; it was dropped on 2026-08-21 for the principle in #558: **offsite backup runs from the store of record, direct.** For Immich that store is the Host: a nightly on-host restic push to a dedicated B2 bucket with an append-only key, specified in #558 and not yet built. What exists today is the absence — the role declares no `backup:` section and `test_immich_meta_declares_no_backup_recipe` in `src/playbook_meta.rs` holds it there. Each granted exception owns its own offsite path and says so on its App page.

## Consequences

**Positive:**

- The stance is checkable. "Does upstream support anything else?" has an answer; "is this bloat?" does not.
- Both prior rejections survive on the new rule's own terms rather than by fiat, each failing a different test — AzuraCast test 1, Actual test 2.
- The unit wrapper means auberge's deploy, status and journal story is unchanged by containerization; the App is an exception in its packaging, not in its operation.
- Research lands before code, with revisit triggers, so a grant expires when upstream's stance changes instead of ossifying.

**Negative / accepted:**

- The daemon's overhead is real and now paid on any Host running a granted exception. The old section's memory reasoning is not refuted, only outranked for one App.
- The `127.0.0.1` rule is enforced by review, not by the firewall. A future exception publishing on `0.0.0.0` would punch through ufw silently — `ss -tlnp` after a deploy is the check.
- The unit's journal carries compose's output, not the containers'. No journald log driver is set, so app logs live in docker's json-file driver and are read with `docker compose logs` — `journalctl -u <app>` is not the whole story it is for a native App.
- Two deployment shapes to understand, document and debug.
- Container Apps sit outside the Backup Recipe world, so the fleet's backup story is no longer answered by one command.
- Grant decisions are judgment calls at the margin. Test 1 in particular rests on what "required" means for a given App, which is the operator's call to make and to write down.

## References

- `meta/adr.md` §"Native systemd by default" — the foundational default this ADR grants exceptions to.
- `meta/research/immich-native.md` — the primary-source evidence for Immich's grant, with revisit triggers.
- ADR-0001 — Backup Recipes, whose executor a granted exception steps outside of.
- ADR-0017 — where the pinned App Version is declared.
- ADR-0020 — AzuraCast, rejected on test 1.
- ADR-0016 — Actual Budget, rejected on test 2.
