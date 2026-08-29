# ADR-0052: The tailnet's global resolver is the Host's own Blocky

## Status

Accepted, 2026-08-29. Supersedes the first decision bullet of [ADR-0003](./0003-tailnet-only-app-dns.md); the rest of that ADR stands.

## Decision

**Headscale pushes the Host's Blocky as the tailnet's `global` nameserver, and pushes nothing beside it.** `headscale-config.yaml.j2` renders `dns.nameservers.global` from `headscale_tailscale_ipv4`, discovered at deploy time from `tailscale status --json`, and falls back to `1.1.1.1`/`1.0.0.1` only while the Host holds no tailnet IP.

`headscale_split_dns_target_ip` keeps its meaning and its auto-derivation, and is no longer how filtering reaches a client. It is the operator's mechanism for pointing one domain somewhere else.

One probe feeds both. It used to run only when the operator had left the split target empty, because deriving that target was the only thing it fed; leaving that guard in place would have made a hand-set split target silently disable the global filter.

**A Host that already serves a tailnet resolver fails the deploy rather than rendering the fallback over it.** `tailscale status` reports "this Host has no tailnet IP" and "I cannot answer" identically — a stopped `tailscaled`, a missing binary and an unenrolled Host all leave the fact unset — and only the first is a reason to publish a public resolver. The config already on disk separates them: a Host serving something outside `headscale_fallback_nameservers` was enrolled when it was last rendered, so the probe's silence is a broken answer, not an absent one. The role reads that config with the `stat`-then-`slurp` pair it already uses for the installed version, and refuses.

Without it the fallback is a second silent-unfiltering path into the same failure this ADR exists to close, reached through the repair deploy rather than the migration: an operator restarting a wedged `tailscaled` and redeploying would publish `1.1.1.1` to every enrolled client and be told nothing.

## Why

ADR-0003 considered this exact shape and rejected it:

> **Headscale-side global Blocky push (no split-DNS).** Strongest filtering coverage but makes Blocky a single point of failure for _all_ DNS resolution including external sites. Rejected: blast radius of a Blocky outage too large.

That blast radius has been the tailnet's live configuration the whole time. The Tailscale SaaS admin console pushes Blocky globally, which `tailscale dns status` reads back as resolver `100.101.255.46` with no split routes at all. ADR-0003 wrote down a design for the headscale template and left the SaaS tailnet running the alternative it had rejected, so the two never had to agree — headscale had zero nodes (#510).

Migrating to headscale is where they meet, and the direction of the mismatch is the problem. The template was not the status quo waiting to be adopted; adopting it would have moved every client from filtered to unfiltered general DNS. Silently, because a tailnet whose ad-blocking stopped working looks exactly like a tailnet that works: the split route would have kept `*.{{ domain }}` resolving, so every internal app, every deploy-time DNS check, and every reachability test a migration runbook would think to run all pass. What breaks is ads loading on unrelated sites, which nothing in this repo observes.

The rejection reasoning was not wrong about the cost. It was reasoning about a cost the operator had already chosen to pay, without knowing that.

### What it costs

**Blocky is load-bearing for every tailnet query, not just internal ones.** ADR-0003's stated mitigation — "split-DNS keeping general internet DNS on `1.1.1.1` — Blocky outage doesn't break browsing, only internal apps" — is gone. A Blocky outage now takes DNS down for every enrolled device.

Accepted, on the grounds that it is measured rather than predicted: this is what the tailnet has done since before ADR-0003, across the whole life of the SaaS tailnet, and the outage it describes has not happened. Blocky is on the same Host as headscale, so a Host that can serve the control plane can serve the resolver.

**The config is one deploy behind the Host's own enrolment, and the window is now tailnet-wide.** ADR-0003's "first-deploy ordering caveat" already named the two-pass shape — `infrastructure.yml` runs `headscale` before `tailscale`, so the probe reads the tailnet IP the Host had when the play started. Two consumers now inherit it, and the second one is not a caveat of the same size:

|                              | old IP still rendered | broken until the next deploy |
| ---------------------------- | --------------------- | ---------------------------- |
| split route (ADR-0003)       | `*.{{ domain }}`      | internal apps                |
| global nameserver (this ADR) | everything            | all DNS on the tailnet       |

On the #510 flag day the Host leaves one tailnet and joins another, so its IP changes from a SaaS `100.101.255.46` to a headscale-allocated `100.64.x.x` — both inside `100.64.0.0/10`, so nothing in the rendered value distinguishes a stale answer from a live one. Between the Host's re-enrolment and the next headscale deploy, the tailnet's global resolver is an address that no longer answers.

The handling is the same one Blocky's own bind address has always had, and it is a sequencing rule rather than a mechanism: **run the infrastructure play again after the Host re-enrols.** The second pass re-probes, re-renders, and notifies `Restart headscale`; Blocky re-binds on the same pass. #712 carries the step.

## Alternatives considered

- **Keep ADR-0003's split-only push.** Rejected: it is the silent regression above. It is also not what the operator has been running, so "preserve current behaviour" argues against it, not for it.
- **Push Blocky _and_ a public resolver globally.** Rejected, and it is worse than it looks. A second entry is not a standby — a client is free to ask either, so a fraction of queries leave unfiltered at random, and the filter's effectiveness becomes a thing you cannot state. It also does not buy the availability it appears to: an outage that takes Blocky down usually takes the Host down, and the Host is where headscale answers from.
- **Declare the global nameserver as a config key instead of discovering it.** Rejected: it is a value the deploy already knows and the operator would have to re-enter after every enrolment change, which is exactly the two-truth split ADR-0003 removed from Paperless. The discovery is the same six lines the `blocky` role uses for its own bind address.
- **Detect a _stale_ IP and refuse to render it.** Rejected, and it is worth separating from the refusal above, which is not this. A stale answer and a live one are both CGNAT addresses in `100.64.0.0/10` from a `tailscaled` that is running and replying; nothing in the probe's output says which tailnet answered, so the guard could never fire. _Absent_ versus _failed_ is a different question with a different witness — the deployed config — and that one is answerable, which is why it is implemented and this is not.
- **Warn instead of failing.** Rejected: a warning in a deploy that reports success is how the SaaS-to-headscale regression would have shipped in the first place. The whole subject of this ADR is a DNS change nothing observes.
- **Reorder `infrastructure.yml` to run `tailscale` before `headscale`.** Rejected: headscale is `tailscale up --login-server`'s server. Running the client first inverts a real dependency to save a second pass that is needed on the very first deploy regardless.
