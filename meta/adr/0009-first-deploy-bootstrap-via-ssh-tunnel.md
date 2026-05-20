# ADR-0009: First-Deploy Bootstrap orchestrated via auberge-driven SSH tunnel

## Status

Accepted, 2026-05-20.

## Decision

Introduce a **First-Deploy Bootstrap** concept: an optional declaration on a Playbook Meta that an App requires a one-time interactive setup step before public DNS Publication can complete. `auberge deploy` orchestrates this step automatically — spawning a local SSH tunnel to the wizard port, opening the operator's browser, polling for a **Bootstrap Marker** file on the remote, and re-running the same playbook to land Caddy + DNS once the marker appears.

Apps that need this declare:

```yaml
first_deploy_setup:
  port: 53842
  marker_path: /var/lib/gokapi/config/config.json
  setup_url_path: /setup
  wizard_name: Gokapi setup wizard
```

Only Gokapi declares it today. The concept generalises to any future App with the same "unauthenticated bootstrap endpoint + post-setup marker file" shape (vaultwarden's first-admin signup, n8n, etc.).

## Why

Gokapi (#338) exposes an unauthenticated `/setup` endpoint on first boot. Whoever reaches `/setup` first creates the admin account, so it must not be publicly reachable before the operator finishes the wizard. The Ansible role gates Caddy + DNS Publication on `/var/lib/gokapi/config/config.json` existing on the remote — the marker the wizard writes when it completes.

Before this ADR, the operator's workflow was:

1. `auberge deploy` — installs binary + service. Gated tasks skip silently. Deploy reports success even though DNS Publication didn't happen.
2. Read the playbook's `debug` task output (easy to miss in rolled-up output) to find the SSH tunnel command, run it manually, complete the wizard.
3. `auberge deploy` — second invocation lands Caddy + DNS.

Three problems:

- Step 1's "success" message is wrong by CONTEXT.md's definition of DNS Publication ("a deploy that completes without a working DNS answer is treated as a failure"). The operator has to read the debug message to know the deploy is half-done.
- Step 2's incantation is forgettable and reinventable per-app.
- Step 3 doubles the deploy command count for the lifetime of every gated App.

Pulling the orchestration into auberge collapses the three steps into one `auberge deploy` invocation while preserving the security posture exactly.

## Security posture

The auto-tunnel matches the manual tunnel byte-for-byte:

- UFW continues to default-deny inbound on the wizard port. The wizard is never publicly reachable.
- Tunnel traffic flows inside the SSH-encrypted channel; UFW only ever sees a connection on port 22.
- Wizard authentication is delegated to SSH key auth (same trust set as the operator's existing access).
- The tunnel is ephemeral — RAII-guarded for teardown on completion, panic, or Ctrl+C.

The change moves a step from the operator's shell history to auberge's process tree. No new attack surface; no new public exposure; no new long-lived state.

## Considered alternatives

- **Template `config.json` directly and skip the wizard entirely.** Rejected for now. Auberge's Config already knows the admin username and password, so a templated `config.json` could land in one deploy with zero interaction. But the operator loses the wizard's optional surface (OAuth providers, storage backends, smtp config) unless every field is added to the Key Registry, and any upstream Gokapi schema change silently breaks the template. Reasonable to revisit if a CI-driven disaster-recovery flow ever needs unattended Gokapi rebuilds — at that point the schema-coupling cost is worth paying.

- **Bind the wizard to the Tailscale interface during bootstrap.** Rejected. Would require Gokapi to support per-interface binding (not verified) and reuses the same trust set as SSH anyway. The SSH path is already there and standard; no new dependency on the mesh being up before the App is.

- **Time-boxed public exposure: open the firewall + create DNS for N minutes, then revert.** Rejected as actively unsafe. Even a 5-minute window of the unauthenticated wizard on the public internet is a race condition the operator can lose to a port scanner.

- **Keep the manual tunnel, just make the instructions louder.** Rejected. The problem isn't discoverability of the instructions; the problem is that two deploy invocations are needed for one deployment, and the friction compounds linearly with the number of Apps that adopt the pattern.

## Consequences

**Positive:**

- One `auberge deploy` invocation completes the deployment, including DNS Publication, for an App with a First-Deploy Bootstrap.
- The Playbook Meta is the single source of truth for the bootstrap shape — no per-App orchestration code in `deploy.rs`.
- Future Apps with the same shape (vaultwarden first-admin, n8n, gitea bootstrap-token) reuse the mechanism by adding a single block to their meta.
- Non-interactive deploys (no TTY — CI, scripts) skip the auto-tunnel and print the manual instructions instead, preserving today's behaviour for unattended runs.

**Negative:**

- Auberge gains a runtime dependency on `xdg-open`/`open`/`start` (via the `open` crate) for browser launch. Failure to open a browser falls back to printing the URL, so this is graceful, but it's one more thing that can not-quite-work on exotic environments.
- The `gokapi` role's `debug` task with manual instructions (lines 156-173) becomes mostly dead weight when auberge drives the flow. Left in place for now to support raw `ansible-playbook` invocations outside auberge.
- Polling the marker file via SSH every 3s during the wizard window means a few dozen SSH connections per bootstrap. SSH connection multiplexing (`ControlMaster auto`) is already configured in `src/ssh_session.rs`, so the cost is one TCP handshake amortised over all the polls.

## References

- Issue #343 — feature request.
- ADR-0008 — introduces Gokapi as the App that motivated this concept.
- Role: `ansible/roles/gokapi/tasks/main.yml:132-173` — the gating logic this ADR wraps with auberge orchestration.
