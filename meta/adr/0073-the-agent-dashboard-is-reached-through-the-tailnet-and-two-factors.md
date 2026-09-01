# ADR-0073: The agent dashboard is reached through the tailnet, behind two factors

## Status

Accepted, 2026-09-01. Decided in #740, implementing [ADR-0054](./0054-agent-workloads-run-on-a-dedicated-disposable-host.md)'s control plane. Depends on [ADR-0071](./0071-a-tailnet-only-apps-parent-domain-is-per-app.md) for the name and [ADR-0072](./0072-the-agent-tiers-caddy-answers-for-its-own-zone.md) for the certificate.

## Decision

**`aoe serve` binds loopback and Caddy is the only thing in front of it**, at `essaim.{agents_domain}`, bound to the Host's tailnet address and nothing else. The Playbook Meta declares `tailnet_only: true`, so the name exists in Blocky's `customDNS` map and in no public zone.

**`--remote` is never passed, and neither is any flag that steers it.** It spawns a public Tailscale Funnel — or falls back to a Cloudflare quick tunnel — and hands out an internet-reachable URL. That is public ingress into the one Host the fleet assumes is compromisable, arriving past Caddy, past the tailnet and past the ACL. It is the only flag in the fleet that can undo a Meta's `tailnet_only: true`, which is why its absence is fenced rather than merely intended.

**Authentication is the URL token _plus_ the passphrase, not the passphrase alone.** The unit passes `--auth token` (upstream's default, stated explicitly) and supplies `--passphrase` through `AOE_SERVE_PASSPHRASE` in a `0600` `EnvironmentFile`. `--auth passphrase` — which #740's triage brief called for, reading it as additive — is **tokenless** upstream: it removes the URL token rather than adding to it, and upstream documents that local TUI attach against such a daemon is unsupported. Only `aoe_passphrase` is pinned from config; the token is server-generated, rotates every four hours and has no flag or environment variable at all, so `aoe_token` is not a config key.

**Caddy's access log for this vhost drops the whole query string.** The bearer token rides in `?token=`, and Caddy redacts `Authorization` and `Cookie` on its own and nothing else.

**The dashboard is a lingering user unit** in the admin user's own systemd manager, `WantedBy=default.target`, so sessions outlive both a client detach and an SSH disconnect. It declares `MemoryHigh` and deliberately no `MemoryMax`.

**No `backup:` section**, held by a test on the immich precedent. A rebuild is a re-auth and a re-clone; the push keypair is unbacked-up state by choice.

## Why

Every one of these fails in a way that reads as success.

**The rebinding gate.** aoe trusts loopback, routable IP literals and its own `--host`, and rejects any other `Host` header. A vhost without `--allowed-host` gets a real certificate, terminates TLS, forwards the request, and then 421s it — DNS resolves, the cert is valid, the proxy is up, and the dashboard is unreachable. `--allowed-origin` is the same story one layer up, for the browser's `Origin`.

**The two factors.** Upstream's own pairing model is a token in the QR URL plus a passphrase on the login page: "either alone is useless." Reading `--auth passphrase` as additive gets one factor and calls it two, and takes local TUI attach out with it. The brief's `aoe_token` key could not have worked at all — there is nothing to set it with.

**The log scrub.** The obvious spelling of "drop the query" is `regexp "\\?.*$" ""`, and it logs `"uri": ""`. Caddy's Caddyfile lexer does not collapse `\\` inside quotes, so Go's regexp receives "an optional literal backslash", matches at offset zero, and eats the path along with the token. Both spellings hide the secret; only `"[?].*$"` leaves a log that says which request was served. Measured against caddy directly, not reasoned about.

**The kill line.** `MemoryHigh` throttles and reclaims; `MemoryMax` is where the kernel kills. aoe supervises tmux sessions running several agents on an 8 GB box, and whether those land in the unit's own cgroup has not been observed. A `max` guessed low OOM-kills an agent mid-run — which is exactly the failure the budget was meant to prevent, arriving from the mechanism meant to prevent it. `Option<String>` on the Budget is what lets the Meta say "throttle, do not kill" rather than saying nothing.

## Trade-off

- **The passphrase is the only credential the operator holds.** The token cannot be pre-seeded, so first pairing needs `aoe url` over ssh — a bootstrapping step no other App has.
- **An installed PWA binds to its exact origin**, so `essaim.{agents_domain}` is frozen once anyone installs it. ADR-0068 already carries this; it is restated here because this is the ADR that names the origin.
- **The dashboard is unreachable off the tailnet, deliberately**, including from a phone with no tailnet client. That is the trade ADR-0054 already made.
- **A user unit is outside two fleet models**: `install_notifies_restart`'s dest→unit model and `service_directories`'s strict-unit scan both read system-manager units only, so aoe's notify edge is a declared one (`DECLARED_ROLES`) rather than a computed one.

## Alternatives considered

- **`--remote` with a Tailscale Funnel**, which upstream recommends for phone access. Rejected: a Funnel is public HTTPS ingress, and ADR-0054's whole premise is that this Host reaches nothing and nothing reaches it except over the tailnet. It would also make the PWA's origin a `.ts.net` name owned by a vendor the fleet left in #712.
- **`--auth passphrase`, as the brief specified.** Rejected on upstream's own documentation: one factor, and local TUI attach unsupported.
- **`--passphrase` on the `ExecStart` line.** Rejected: argv is readable out of `/proc` by every process on a Host that runs unattended agents. The unit file is `0644` besides.
- **Delete only `token` from the logged query** (`query { delete token }`). Rejected: it names the parameter, so a second sensitive parameter upstream adds later is logged in full until someone notices. Dropping the query whole costs nothing here — no query parameter on this vhost is worth logging.
- **Bind aoe on the tailnet address and skip Caddy.** Rejected: PWA install and web push both require a secure context, so HTTPS is functional rather than cosmetic — and a second listener would be a way in that Caddy neither terminates TLS for nor logs.
- **A `backup:` section covering `~/.agent-of-empires`.** Rejected: it would put a nightly stopped-unit pull of a deliberately disposable Host into `backup sync`'s default app set, and the one piece of unrecoverable state in it (the VAPID keypair) costs a re-subscribe.
- **Set `MemoryMax` at some safe-looking multiple of `MemoryHigh`.** Rejected as a guess about a cgroup placement nobody has looked at. Observe first; a follow-up sets it.
