# ADR-0009: Gokapi headless first-deploy via templated config + --deployment-password

## Status

Accepted, 2026-05-20.

## Decision

Skip Gokapi's setup wizard entirely on first deploy. Auberge templates `config.json` with the operator's settings and runs `gokapi --deployment-password '<pw>'` once before the systemd service starts. Gokapi's existing non-interactive bootstrap path creates the superadmin in its database; subsequent service starts find a complete configuration and never open the wizard webserver.

Caddy site and DNS Publication deploy unconditionally in the same `auberge deploy` invocation — no gating on a marker file, no second deploy, no SSH tunnel.

## Why

The setup wizard is gated on `!configuration.Exists()` (`cmd/gokapi/Main.go:57` calls `setup.RunIfFirstStart()`, which only starts the wizard webserver if no config file is on disk — `internal/configuration/setup/Setup.go:60-65`). Pre-writing `config.json` makes the wizard a no-op.

Gokapi separately ships `--deployment-password`, which `internal/configuration/database/Database.go:312-337` documents as: _"EditSuperAdmin changes parameters of the super admin. If no user exists, a new superadmin will be created."_ The startup error message in `cmd/gokapi/Main.go:196` advertises this path explicitly: _"No user found in database. Please run setup first or create user with `--deployment-password`."_

Both halves of the wizard — writing the config file and creating the superadmin — are first-class CLI operations. The wizard exists for operators who don't run a configuration-management tool. Auberge does. There is no reason to drive the wizard from auberge.

A prior iteration of this work (closed in PR #344) proposed automating an SSH tunnel + browser launch so the operator could complete the wizard programmatically. That added: a new domain concept (First-Deploy Bootstrap), a new service module (~200 lines of Rust), an `open` crate dependency, a polling loop, an RAII tunnel guard, and per-tag meta dispatching in the deploy command. None of that orchestration is needed once you read Gokapi's source.

## How

The implementation is ansible-only. Auberge's Rust deploy command is unchanged.

**1. Template `config.json` once per host.** A new `roles/gokapi/templates/config.json.j2` writes the minimum-viable configuration shape — `Authentication.{Method=0,Username}`, `Port`, `ServerUrl`, `DataDir`, `DatabaseUrl`, `ConfigVersion=22`, `Encryption.{Level=0}`. The ansible task uses `force: false` so subsequent deploys never clobber operator-applied changes from the admin UI (which Gokapi persists to the same file via `configuration.save()`).

**2. Run the password one-shot once per host.** A new task invokes `{{ gokapi_install_path }}/gokapi --deployment-password '{{ gokapi_admin_password }}'` as the `gokapi` system user. The one-shot is gated on _two_ conditions: the bootstrap marker is absent AND no pre-existing `config.json` was on disk before this deploy ran. The second guard handles hosts that were previously bootstrapped via the wizard — they have a valid superadmin in the database but no marker file, and `EditSuperAdmin` would otherwise overwrite the existing password on the next deploy. For such hosts the role auto-touches the marker (without invoking gokapi) so subsequent deploys are idempotent. Gokapi exits 0 immediately after `EditSuperAdmin` finishes; a subsequent task creates the marker on hosts that did run the one-shot.

**3. Start the systemd service.** Now safe: config exists, superadmin exists, `checkIfUserExists` (`cmd/gokapi/Main.go:194-199`) passes, gokapi enters its normal serving loop.

**4. Deploy Caddyfile and DNS A record unconditionally.** The `when: gokapi_config_check.stat.exists` gates on those two tasks are removed. The first-deploy debug task (lines 156-173 in the previous version) is removed.

## Security posture

Strictly better than both the manual wizard flow and the auto-tunnel flow proposed in PR #344:

- `/setup` is never reachable from anywhere. `RunIfFirstStart` returns immediately because config exists, so the setup webserver never binds its port at all. Compare with the wizard flow where the setup webserver listens on the wizard port and only network reachability prevents external access.
- The admin password lives in `auberge config` (already a secret per the Key Registry) and is passed to Gokapi via a single CLI invocation. It does not traverse the network during bootstrap. Compare with the wizard flow where the operator types the password into a browser tab tunneled through SSH.
- UFW continues to default-deny inbound on the wizard port; this remains true and is now also moot.

## Considered alternatives

- **Auto-tunnel + browser + wizard (PR #344).** Rejected after reading Gokapi's source. The wizard isn't load-bearing — it's a convenience interface for operators without a CM tool. Reproducing its work in Rust (orchestrating SSH, polling a marker, opening a browser, handling non-interactive mode) is fighting against a documented non-interactive escape hatch. Same outcome with ~200 lines fewer of orchestration code and one fewer dependency.

- **Keep the wizard, document the SSH tunnel.** Rejected. The original status quo: one deploy per app first-time, plus an operator who has to remember an `ssh -L` incantation. The friction was the entire motivation for the work; not solving it is not an answer.

- **Template `config.json` with a pre-computed password hash, skip `--deployment-password`.** Rejected. Gokapi's password hash format (`HashPassword(pw, false, "")` in `internal/configuration/Configuration.go:199`) couples us to its internal KDF. Re-implementing it in ansible/Jinja is brittle. Letting Gokapi hash via its own `--deployment-password` flag keeps the KDF as Gokapi's concern.

- **Skip the marker, run `--deployment-password` every deploy.** Rejected. Works (idempotent — `EditSuperAdmin` updates the existing superadmin if one is found), but writes to the database on every deploy for no reason. Marker is one `ansible.builtin.file: state: touch` and pays for itself.

## Consequences

**Positive:**

- One `auberge deploy` invocation completes the deployment, including DNS Publication, for Gokapi.
- The `gokapi` role's task list is ~10 lines shorter (no debug task with manual instructions, no `when: gokapi_config_check.stat.exists` gates).
- No new Rust code, no new auberge dependency, no new domain concept. The complexity budget is preserved for problems that actually need it.
- The pattern (template config + `--deployment-password` one-shot + marker) generalises to any future App whose authors built a similar non-interactive bootstrap path. Each such App's role declares it directly; no shared abstraction needed at the auberge level until two or three Apps demonstrate the same shape.

**Negative:**

- Template coupling to Gokapi's `config.json` schema (`ConfigVersion: 22` for Gokapi 2.2.4). A future version bump that changes the schema breaks the template until `gokapi_version` and `ConfigVersion` are bumped together. Mitigated by Gokapi's `configupgrade` module (`internal/configuration/configupgrade/Upgrade.go`), which catches schema mismatches with a loud error rather than silent misbehaviour.
- Operators who change `gokapi_admin_password` in their config and re-deploy will not see the new password applied until they delete the bootstrap marker on the host (`gokapi_bootstrap_marker`, default `/var/lib/gokapi/.bootstrap_done`) and re-deploy. Rotating the password is a deliberate operator action, not a deploy side effect. Documented in the role's `README.md`.
- The operator loses the wizard's "click through optional features" UX surface (OAuth provider setup, S3 backend selection, end-to-end encryption modes). Per the operator's stated workflow (ADR-0008: "drop file, share URL"), default internal auth + local storage is the right configuration. If OAuth or S3 is ever needed, `gokapi --reconfigure` re-opens the wizard with random throwaway credentials printed to stdout (`internal/configuration/setup/Setup.go:67-80`) — a one-off SSH session, not a per-deploy concern.

## References

- Issue #345 — implementation issue.
- PR #344 (closed) — the abandoned auto-tunnel approach. Closed when the headless path was discovered in Gokapi's source.
- Issue #343 (closed) — superseded by #345.
- ADR-0008 — establishes the operator's Gokapi use case as "drop file, share URL," which informs the default-internal-auth + local-storage choice in the template.
- Role: `ansible/roles/gokapi/tasks/main.yml` — implementation lives here.
- Gokapi source: `cmd/gokapi/Main.go:57-64`, `internal/configuration/setup/Setup.go:60-65`, `internal/configuration/database/Database.go:312-337`, `internal/configuration/Configuration.go:192-209`.
