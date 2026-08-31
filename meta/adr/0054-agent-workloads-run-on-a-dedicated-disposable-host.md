# ADR-0054: Agent workloads run on a dedicated disposable Host

## Status

Accepted, 2026-08-31. Decided in #747.

## Decision

**AI agent workloads run on `ruche`, a dedicated Host rebuilt from the paid-up OVH box (4c/8GB, term ends 2027-08-20), holding only state reproducible from IaC.** The stack, layer by layer, chosen so no single vendor sits in the control path:

- **Substrate + phone control plane**: agent-of-empires — agent-aware tmux sessions (idle/working/blocked/done), worktree-per-agent, and a web dashboard served tailnet-only at `essaim.{domain}` as an installable PWA with push notifications and swipe-to-approve. HTTPS via caddy is functional, not cosmetic: PWA install and web push require a secure context.
- **Runtimes**: OpenCode primary (unattended lane — permission config of `"*": "allow"` plus an explicit deny list, models via OpenRouter), pi secondary (attended/scriptable lane, print/RPC modes). Runtimes are a dependency list, not a commitment: adding or dropping one is one line in the meta role.
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
- Claude remote-control (the only first-party phone loop) is foregone. Noted for any future reuse: the `claude_code` role writes `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` to `/etc/environment`, which disables remote-control on any host the role touches.
- The box expires 2027-08-20 — a forced re-home, priced at `auberge deploy ruche -H <next>` precisely because of the no-irreplaceable-state rule.

## Alternatives considered

- **`vibecoder` as committed (Telegram bridge)** — the role cannot run (`claude_code_remote` templates a `.env.j2` that does not exist in the role) and clobbers `~/.claude/settings.json`; a chat thread is a serial pipe with no diffs and no parallel visibility. Deleted by #743.
- **Claude Code + remote-control** — best-integrated steering (Claude app, native push, 32-session server mode) but subscription-auth against `api.anthropic.com` only: vendor lock at both the model and control layers. Rejected on the model-freedom requirement.
- **herdr** — agent-aware multiplexer shipping with Omarchy Quattro, but its phone story is an SSH thin client: no dashboard, no push.
- **Hosted control planes (Omnara, Happy relays, Grok Bot)** — a third-party relay or vendor cloud in the control path of a box designed for zero external trust.
- **Agents stay on auberge beside hermes** — rejected by the blast-radius argument above; hermes remains the companion, not the fleet.
