# ADR-0058: Config answers per Host

## Status

Accepted, 2026-08-31. Decided in #753; the trap #739's triage named. Extends ADR-0051.

## Decision

**`config.toml` reserves a `[hosts.<name>]` table per Host. For that Host's runs, a key in its table overrides the top level — Preflight validation and the `--extra-vars` file both read the merged view. A blank override withdraws a fleet-wide answer for that Host.**

The mechanics compose with three existing contracts instead of adding new ones:

- **The ADR-0051 gate expression is unchanged.** `<key> is defined and <key> | length > 0` now evaluates against per-Host extra-vars, so the same expression answers differently per Host — a blank override reads as unset. The roster gate and any out-of-role guard (the UFW STUN rule) keep reading the identical source, so they still agree by construction.
- **ADR-0045's selection rule is unchanged.** `validate_required` treats a blank value as missing, so `-t headscale` against a Host that blanked the gate fails Preflight naming the key — naming the tag is still the operator asserting the role runs, and the assertion is now checked against the Host it targets. Only the untagged run skips a guarded role's keys, as before.
- **Zone-global facts stay top-level.** An `<app>_subdomain` is the App's record name in the one DNS zone; DNS discovery keeps reading it from the top level. The per-Host table carries _divergence_ — which of the fleet's answers this Host withdraws or replaces — never the fleet's identity.

Two consequences the repo owns:

- The top-level `hosts` table never flattens into a run's extra-vars wholesale: `flatten_for_ansible` skips it and merges only the target Host's entry. Before this, any nested table's leaves leaked into every run under their leaf names.
- `blocky` joins `headscale` behind an ADR-0051 gate in `infrastructure.yml`. It was unconditional, so pointing `infrastructure` at a second Host installed a second resolver racing the `dns.{domain}` certificate — against ADR-0052's sole-resolver rule. The role never defaults `blocky_subdomain` (only derives from it), so the gate is real by the ADR-0051 test.
- `auberge headscale` derives its default Host from the gate: the one Host whose merged view answers `headscale_subdomain` non-blank. Zero or several answers fall back to the picker.

## Why

The infrastructure roster had no per-Host answer, and the fleet stopped being one Host long before ruche: `vieille-auberge` sits in the same roster today. `group_vars/` cannot carry the answer because Preflight passes every config key as `--extra-vars`, which outrank everything Ansible-side — the merge has to happen where the vars file is built. Scoping config rather than inventing a role-selection vocabulary keeps ADR-0051's rule intact: config alone answers a gate; whose config just became precise.

## Trade-off

- **blocky's keys leave the untagged demand set.** Every untagged `infrastructure` run used to require `blocky_subdomain`; now a first deployment that never sets it silently deploys no resolver. Accepted: it is the same semantics ADR-0051 accepted for headscale, blocky is genuinely optional off-tailnet, and `config init`'s full scaffold still offers the key. No fleet-level "at least one Host answers" assertion exists — a per-Host gate has no run to hang it on.
- **A fleet where several Hosts answer the headscale gate gets the picker, not a default.** The deleted `hostname` fallback answered non-interactively; the derived default returns only when exactly one Host serves. The remedy is the mechanism itself: withdraw the gate on the non-serving Hosts.
- **CLI-side restic operations stay fleet-wide.** `backup push`/`verify`/`prune` read `restic_repository`/`restic_password` through the top level; a `[hosts.<name>]` override of them reaches Ansible but not these commands. Per-Host repositories are not a supported shape.

## Alternatives considered

- **Group-based gates** (`when: "'agent' in group_names"` from `hosts.toml` tags). Rejected: hardcodes trust-tier names into the roster, cannot express vieille-auberge (same groups as auberge, different serving set), and splits the gate's answer across two files.
- **A per-Host role roster** (`infrastructure_roles = [...]` per Host). Rejected: a second name for the fact the gate key already states — ADR-0051 rejected exactly that shape once.
- **`hostname`-style derivation.** Nothing to derive from: which Host serves the control plane is genuinely operator configuration, not a consequence of naming.
