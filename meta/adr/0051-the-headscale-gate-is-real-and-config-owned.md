# ADR-0051: The headscale deploy gate is real, and config alone answers it

## Status

Accepted, 2026-08-29. Decided in #710; follows the adoption decision (ADR-0049).

## Decision

**`headscale_subdomain` lives in config only.** The role has no default for it; `roles/headscale/defaults/main.yml` derives `headscale_domain` from it but never answers it. Setting the key deploys headscale and opens 3478/udp; leaving it unset does neither.

Two consequences the repo owns:

- **Both guards now read the same source.** The roster gate (`infrastructure.yml`) and the UFW STUN rule (`roles/ufw/tasks/main.yml`) test the identical expression, and with the role default gone they agree by construction — there is no scope in which one sees an answer the other cannot.
- **Naming the tag demands the key.** `headscale.meta.yml` declares `required_keys: [headscale_subdomain]`. A gate that can be false makes a `-t headscale` run without the key a silent no-op — every task skips — so Preflight asks for it up front, per ADR-0045's rule that naming a guarded role's tag is the operator asserting it runs. Any selecting tag counts — `-t vpn`, `-t network`, `-t infrastructure` demand the key the same way, exactly as `-t ai` already demands hermes' keys: ADR-0045's selection rule is unchanged here, and only the untagged run skips a guarded role's keys. An operator on SaaS pays for that with one config key, or by naming the roles they actually want.

The Meta's `subdomain: hs` stays: that is DNS discovery's default record name, a different concern, and config overrides it through the same key.

## Why

The gate was fake. A role-level `when:` is copied onto every task and evaluated at task runtime, where the role's own defaults are in scope — so a guard testing a variable the role defaults can never be false. The proof was the same expression disagreeing with itself: the UFW STUN rule evaluates _outside_ the headscale role, no role defaults in scope, so with the key unset it could never be **true**. One condition, two permanent opposite answers; live result (#510): headscale deployed on every Host, 3478/udp closed on every Host.

## Alternatives considered

- **Delete the gate — headscale as unconditional substrate.** The fleet just adopted headscale (ADR-0049), so "always deploy it" is defensible and simpler. Rejected: the tailscale role explicitly supports SaaS (`tailscale_login_server` empty selects it), so headscale is genuinely operator-optional, and the gate should mean something. An operator running the roster against SaaS gets no control plane they never asked for.
- **Fix the guard instead of the default** — e.g. gate on a differently-named variable no role defaults. Rejected: it leaves two names for one fact and the trap armed for the next role-level `when:`; removing the default makes the existing expression correct everywhere it appears.
