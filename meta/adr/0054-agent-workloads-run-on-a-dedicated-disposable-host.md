# ADR-0054: Agent workloads run on a dedicated disposable Host

## Status

Accepted, 2026-08-31. Decided in #747. **Amended 2026-09-01 by #743**, which drops the second runtime and records the `claude_code` role's deletion; everything else below stands. [ADR-0075](./0075-the-agent-tier-deploys-as-one-guarded-composition.md) builds the composition this ADR calls "a dependency list".

## Decision

**AI agent workloads run on `ruche`, a dedicated Host rebuilt from the paid-up OVH box (4c/8GB, term ends 2027-08-20), holding only state reproducible from IaC.** The stack, layer by layer, chosen so no single vendor sits in the control path:

- **Substrate + phone control plane**: agent-of-empires — agent-aware tmux sessions (idle/working/blocked/done), worktree-per-agent, and a web dashboard served tailnet-only at `essaim.{domain}` as an installable PWA with push notifications and swipe-to-approve. HTTPS via caddy is functional, not cosmetic: PWA install and web push require a secure context.
- **Runtime**: OpenCode, and only OpenCode — unattended lane, permission config of `"*": "allow"` plus an explicit deny list ([ADR-0065](./0065-the-agent-permission-baseline-is-a-guard-rail-not-a-boundary.md)), models via OpenRouter. Runtimes are a roster, not a commitment: adding or dropping one is one line in `ruche.yml`.
- **Git identity**: a GitHub machine user with a per-repo allowlist; no credential on the box acts as the owner.
- **Memory**: agent transcripts leave the box via syncthing to lechuck; memsearch indexes them on-box. The index is rebuildable, the transcripts survive box death — state disposable by construction.
- **No `backup:` section** in `ruche.meta.yml`, held there by a test (the immich precedent). A rebuild means re-auth and re-clone, nothing more.
- **Isolation**: enrolled as `tag:agent` under the tailnet ACL policy (ADR-0055). The box is assumed compromisable.

## Why

Unattended agents running with permissions bypassed make prompt injection the design assumption, not an edge case — so isolation must be structural, not behavioral. Today hermes shares a kernel with photos, documents, and mail; an agent with shell access and API keys sits beside paperless and bichon. Moving agent workloads to a box that is sunk cost with a known end date, holds nothing irreplaceable, and cannot reach the data Hosts turns "an agent got injected" from an incident into a reinstall.

The control plane is agent-agnostic and the model supply is metered-any-vendor because the two lock-in points observed in the ecosystem are exactly those layers: first-party phone loops bind to one vendor's auth and endpoint, and subscription pricing binds the runtime to one vendor's models. aoe + OpenRouter keeps both swappable.

## Trade-off

- Metered OpenRouter pricing loses to flat-rate subscriptions under heavy frontier-model burn — accepted for vendor freedom; a subscription runtime can be re-added as one dependency line if the economics flip.
- aoe is community-maintained — accepted because the substrate is the most swappable layer (herdr or bare tmux as fallback; sessions are disposable by design).
- Claude remote-control (the only first-party phone loop) is foregone. The `claude_code` role that would have carried it is gone with `vibecoder` (#743) — it had no other consumer, and the trap it held is worth restating for anyone re-adding one: it wrote `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` to `/etc/environment`, which disables remote-control on every host the role touches, and `/etc/environment` is world-readable besides.
- The box expires 2027-08-20 — a forced re-home, priced at `auberge deploy ruche -H <next>` precisely because of the no-irreplaceable-state rule.

## Amendment, 2026-09-01: one runtime

The runtime list above read "OpenCode primary, pi secondary". #742 — the pi role — was closed one-harness-only before it was built, so the second lane never existed and the composition #743 shipped has one runtime in it.

The reason is not that pi is worse. It is that a second runtime buys a second _attended, scriptable_ lane on a Host whose whole premise is unattended work steered from a phone, and pays for it with a second permission baseline to keep honest, a second credential path, and a second answer to "which harness wrote this commit". The vendor-freedom requirement this ADR is built on is satisfied at the model layer by OpenRouter, not at the harness layer by running two — so the second harness was cost without the property it was meant to buy.

What the original bullet claimed stays true and is the reason this is cheap to revisit: a runtime is one line on `ruche.yml`'s roster (ADR-0075).

## Alternatives considered

- **`vibecoder` as committed (Telegram bridge)** — the role could not run (`claude_code_remote` templated a `.env.j2` that did not exist in the role) and clobbered `~/.claude/settings.json`; a chat thread is a serial pipe with no diffs and no parallel visibility. Deleted by #743, along with `claude_code_remote` and `claude_code`.
- **Claude Code + remote-control** — best-integrated steering (Claude app, native push, 32-session server mode) but subscription-auth against `api.anthropic.com` only: vendor lock at both the model and control layers. Rejected on the model-freedom requirement.
- **herdr** — agent-aware multiplexer shipping with Omarchy Quattro, but its phone story is an SSH thin client: no dashboard, no push.
- **Hosted control planes (Omnara, Happy relays, Grok Bot)** — a third-party relay or vendor cloud in the control path of a box designed for zero external trust.
- **Agents stay on auberge beside hermes** — rejected by the blast-radius argument above; hermes remains the companion, not the fleet.
