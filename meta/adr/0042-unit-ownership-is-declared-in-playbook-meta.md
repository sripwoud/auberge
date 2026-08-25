# ADR-0042: Unit Ownership is declared in Playbook Meta

## Status

Accepted, 2026-08-25. **Applies ADR-0028's declared regime to unit ownership** — the fourth fact about a unit the repo cannot fully read off its templates, after restart edges (ADR-0028), directory writability (ADR-0035), and shutdown exit status (ADR-0038).

## Decision

Each App's Playbook Meta declares the systemd units it answers for, under `units:`. A bare name is a `.service` (the Recipe convention, ADR-0032), `{admin_user}` resolves like a Recipe's (ADR-0023), and a unit that lives in a user manager says `scope: user`, because `systemctl show` cannot see it without `--user`.

On a failed deploy — and only there; a successful deploy is unchanged, and check mode changes no unit so it reads nothing out — the CLI resolves the failing run to its Apps (the run's tags, or the whole playbook when untagged), collects their declared units, probes the Host once per scope with a batched `systemctl show`, and appends to the failure it already prints the verdicts an operator acts on differently: **restart-looping** (`activating (auto-restart)`), **failed and gave up** (the Start Limit Regime's verdict, ADR-0040), **stopped**, **restarted mid-deploy**, with everything still active from before the deploy rolled up as untouched.

Two derivation rules keep the report honest:

- "Pre-deploy artifact" vs "restarted mid-deploy" is computed, never asserted: the unit's monotonic start timestamp against the Host's own `/proc/uptime` and the locally measured deploy duration — one clock, so no skew between operator and Host — and the raw `ActiveState (SubState)` stays on every line so a wrong verdict is auditable.
- The probe runs against a host whose deploy just failed, so it can fail too; its error becomes one appended line (`unit state probe failed: …`) and never displaces the deploy failure it annotates.

The declaration is hand-written, so `tests/unit_ownership.rs` fences it the way ADR-0028's descendants fence theirs: everything the roles' own tasks reveal — every unit file a role templates or copies into a unit directory, and every unit a role drops in over, since a drop-in names the unit it refines — is computed and matched against the declarations by set difference in both directions. The one unit no task reveals (syncthing's enabled-only packaged template instance) is declared in `DECLARED_WITHOUT_FILE` with the reason the scan cannot see it, and the fence also proves it _stays_ underivable, so the excuse dies with its cause.

Deliberately outside the domain, pinned by a third test: shared substrate an App merely starts (postgresql, mariadb, redis, docker, tailscaled) — it has its own owners, and dragging it in turns the declaration into a dependency graph, a different feature — and php-fpm, whose unit name (`php8.4-fpm`) is a play-time package fact the yourls role itself discovers by regex over `package_facts`, so a Meta literal would drift on every PHP transition.

## Why

A failed deploy reported the playbook, the exit code, and the last output — not whether the App was serving old code, serving nothing, or restarting in a loop. On 2026-08-22 a grimmory deploy landed a jar missing a directory its app migration hard-requires; the Readiness Probe did its job and failed the play, and grimmory then crash-looped for another seven hours, 4628 starts, invisible: the deploy had already exited, and a unit in auto-restart is `activating`, never `failed`, so `systemctl --failed` had nothing (#642, fixed since by ADR-0040 — which reports the verdict _eventually_, at the Start Limit Regime's pace; this reads the state out at the moment the operator is actually looking).

Reporting needed a runtime answer to "which units does this Playbook own", and auberge had none. The two partial declarations were built for other purposes: a Backup Recipe's `systemd_services` is a _quiesce order_ that only the 11 Apps with a Recipe have, and `memory:` keys are opt-in per Memory Budget. Eight unit-owning Apps had neither (baikal, blocky, caddy, claude_code_remote, colporteur, hermes, immich, radio). The full set existed only at test time, in `tests/shutdown_exit_status.rs`'s scan.

Deriving the inventory instead of declaring it — at build time or from the extracted assets tree at run time — was considered and rejected: derivation is structurally blind to exactly the units that made the gap interesting. `navidrome.service`, `icecast2.service`, and `cockpit.socket` are packaged, revealed only through the drop-ins the roles lay over them; `syncthing@` is enabled and never installed. Any derivation therefore still needs a declared remainder — at which point the repo's proven shape is a declaration fenced by everything derivable, and the drop-in trick shrinks the underivable remainder to a single entry.

Report, not remediate: stopping a looping unit would make the state unambiguous but forecloses recovery when the cause is transient — a dependency slow to come up rather than a bad artifact. The grimmory incident was a telling-failure, not a missing-action failure; if remediation is ever wanted it is its own decision.
