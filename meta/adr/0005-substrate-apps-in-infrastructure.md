# Substrate Apps live in `infrastructure.yml`

## Status

Accepted, 2026-05-06.

## Decision

An App whose deploy state must be present and correct before another App's deploy can verify reachability is **substrate**, and is declared in `ansible/playbooks/infrastructure.yml` rather than `apps.yml`. Currently covers **Caddy** (HTTPS for every App), **Headscale** (login server for Tailscale on first deploy), and **Blocky** (DNS Publication for every Tailnet-only App).

## Why

`auberge deploy <app>` runs the `apps` play with `--tags <app>`, executing only the named role. A substrate role living in `apps.yml` is silently skipped, so the named App can deploy "successfully" while a sibling whose side effect its reachability depends on never runs. Bichon's `mail.sripwoud.xyz` resolving NXDOMAIN despite a green deploy (issue #312) is the canonical instance of the bug class this decision closes.

Roles in `infrastructure.yml` run unconditionally on every deploy, so substrate state stays in sync with App declarations without CLI-side or operator-side coordination.

## Considered alternatives

- **Auto-include the substrate's tag in the Rust CLI** when a dependent App is being deployed. Rejected: spreads orchestration knowledge between Rust and Ansible; new substrates must be hard-coded in the CLI; direct `ansible-playbook` invocations hit the same gap.
- **Per-App filesystem-drop pattern for every substrate** (Caddy-style: each App role drops `/etc/<substrate>/sites/<app>.snippet`, substrate role's config is frozen). Works for Caddy because Caddy supports wildcard imports. Not a 1:1 swap for Blocky: `customDNS.mapping` is a flat YAML map without per-file imports; switching to Blocky's `hostsFiles` directive narrows expressible record types. A drop-pattern refactor remains a per-substrate option in the future; this ADR does not block it.

## Consequences

- Every `auberge deploy <app>` runs the full `infrastructure.yml` play. Per-role overhead is bounded; idempotent roles produce no-op runs when state is unchanged.
- "Has a subdomain" does not imply placement in `apps.yml`. Headscale (`hs`) and Blocky (`blocky`) have subdomains and are substrate. The CONTEXT.md App taxonomy (App / Public App / Tailnet-only App) is unchanged; placement is governed by this ADR independently.
- New substrate components must be declared in `infrastructure.yml`. Criterion: "another App's deploy reachability check depends on this being correct."
