# ADR-0045: Required config keys are declared in Playbook Meta

## Status

Accepted, 2026-08-27. **Completes the trajectory ADR-0017, ADR-0021 and ADR-0042 set** — App Versions, Memory Budgets and Unit Ownership all declare in the Playbook Meta; `required_keys` was the one section that declared there and was read somewhere else.

## Decision

Preflight validation resolves a run's required config keys from the Playbook Metas. The effective set is a union: the Playbook's own Meta declarations, plus the Metas of the roles the selected tags resolve to through that Playbook's own roster. The hardcoded per-playbook match and the per-tag table in `config.rs` are deleted.

Three rules make the union answerable:

- **An untagged run resolves every unguarded roster role.** It enters the whole roster, so it demands what the whole roster needs — the case the drift left failing mid-play, and the one a category tag cannot cover. The exception is an entry carrying a `when:`, which turns on Host facts no caller can evaluate before the play runs: `hermes` carries `when: "'hermes' in group_names"` and `headscale` a `headscale_subdomain is defined`, so demanding their keys would fail every Host that never enters them. A guard makes a role's keys unaskable, not optional — naming its tag is the operator asserting it runs, and then they are asked.
- **A tag selects a role when the tag is one of the role's declared tags, or is the role's own name.** This is what gives category tags (`media`, `web`, `storage`) their first real validation: `--tags media` now resolves calibre, grimmory, immich, navidrome and radio and demands what all five need.
- **An App that is also a standalone playbook declares the shared base itself.** `gokapi.yml` and `immich.yml` are runnable directly, where `apps.meta.yml` is not in the union at all. Their Metas therefore carry `domain` and `cloudflare_dns_api_token` alongside their app-specific keys; roster-only Apps declare app-specific keys only, and the redundancy costs nothing because the union deduplicates.

Every declared name is checked against the Key Registry at the point the Meta is read, so a typo fails at resolution with the file named rather than surviving to a mid-play undefined-variable error. The check lives in the resolver rather than in `PlaybookMeta::load`, because the Meta's other consumers — Backup Recipes, DNS subdomains, Unit Ownership — have no business holding a Key Registry to read a section they never touch.

Four fences hold the shape, in `tests/required_keys_declarations.rs`: every `apps.yml` roster role has a Meta; every playbook has a Meta, because a playbook without one validates nothing at all and reports a clean Preflight for it; every `required_keys` name across every Meta exists in the Key Registry, which was true before this change only by luck; and every roster tag selects at least one role, so a typo is a no-op run rather than a silently unvalidated one. They read the tree as text through `tests/common/mod.rs`, independently of the resolver whose unit tests prove the union itself.

`Config` keeps sole authority over constructing a `Preflight` (`preflight_with_keys`), so the capability that unlocks `AnsibleRunner` still cannot be forged; only the resolution moved out, to the layer that can see the roster.

## Why

The two authorities had already disagreed, in both directions, and neither could be trusted to be the drift-free one.

`config.rs` doc-commented its match "the Key Registry — the single authoritative source of which config keys each playbook needs", while the Metas' `required_keys:` sat unread on the deploy path — declared data with no consumer, which is indistinguishable from a comment. `auberge ansible run gokapi` validated `admin_user_name` and `domain` from a `_ =>` fallback arm and nothing gokapi-specific, despite `gokapi.meta.yml` having declared the admin credentials all along.

A per-key grep of all 62 Key Registry names against every role on the `apps.yml` and `infrastructure.yml` rosters, taking a role's own `assert` or an unguarded reference with no default as proof of requirement, found the drift was not a handful of keys but 30 declarations across twelve roles: all five of yourls' secrets, paperless' four asserted keys, grimmory's asserted four, bichon's two, baikal's admin password, caddy's ACME address, and nine `<app>_subdomain` keys with no default anywhere. Every one of them failed mid-play before, after the deploy had already begun changing the Host. Fail Fast is a stated principle of this repo; a preflight that validates three keys and lets 30 through is not one.

One gap the grep found is deliberately left open: `calibre_subdomain` is required by the calibre role exactly as `colporteur_subdomain` is, but it is not in the Key Registry at all, so it cannot be declared without first being a key. That is a different defect — a role reading a variable the Registry never names — and is tracked separately.

The audit also overturned the assumption the work started from. `colporteur_subdomain` looked like the clear over-enforcement — the hardcoded table demanded it, `colporteur.meta.yml` declared nothing, and the Meta carries `subdomain: colporteur`. But that field is a CLI-side default for DNS record management; nothing injects it into Ansible, and the role has no default of its own. Dropping the key would have moved a working deploy's failure from Preflight to an undefined variable. "Meta wins" is right as a rule for where the fact lives, not as a licence to treat the current Meta as correct: the grep decides, and here it added the key to the Meta instead of deleting it from the enforcement. One symmetric case ran the other way — `immich.meta.yml`'s missing `admin_user_name` is correct, because the immich role never reads it.

Deriving the requirements instead of declaring them was considered: the audit script is most of a derivation already. It was rejected as a runtime authority for the reason ADR-0042 rejected deriving unit ownership — derivation is blind in exactly the interesting places. `hostname` inside `<hostname>{{ radio_domain }}</hostname>` is an XML tag, not a variable reference; `actual_tailscale_ip` is a Key Registry name a role sets by `set_fact` from the Tailscale API and only accepts as an override; `baikal_busy_icloud_*` is optional behind `is defined`. A scanner that cannot tell these apart either demands keys nobody needs or misses keys everybody does. The scan's value is as an audit run by a person against a declaration, which is what it was here.
