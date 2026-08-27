# ADR-0045: Required config keys are declared in Playbook Meta

## Status

Accepted, 2026-08-27. **Completes the trajectory ADR-0017, ADR-0021 and ADR-0042 set** — App Versions, Memory Budgets and Unit Ownership all declare in the Playbook Meta; `required_keys` was the one section that declared there and was read somewhere else.

## Decision

Preflight validation resolves a run's required config keys from the Playbook Metas. The effective set is a union: the Playbook's own Meta declarations, plus the Metas of the roles the selected tags resolve to through that Playbook's own roster. The hardcoded per-playbook match and the per-tag table in `config.rs` are deleted.

Three rules make the union answerable:

- **An untagged run resolves no roles.** It reads the Playbook's Meta and stops. Unioning the whole roster would be the honest reading of "what runs" only if the whole roster ran everywhere, and it does not: `hermes` carries `when: "'hermes' in group_names"`, so a no-tag deploy of any other Host would be told to set `hermes_llm_api_key` for a role it never enters. The shared base (`admin_user_name`, `domain`, `cloudflare_dns_api_token`) is declared once in `apps.meta.yml` and so still covers that case.
- **A tag selects a role when the tag is one of the role's declared tags, or is the role's own name.** This is what gives category tags (`media`, `web`, `storage`) their first real validation: `--tags media` now resolves calibre, grimmory, immich, navidrome and radio and demands what all five need.
- **An App that is also a standalone playbook declares the shared base itself.** `gokapi.yml` and `immich.yml` are runnable directly, where `apps.meta.yml` is not in the union at all. Their Metas therefore carry `domain` and `cloudflare_dns_api_token` alongside their app-specific keys; roster-only Apps declare app-specific keys only, and the redundancy costs nothing because the union deduplicates.

Every declared name is checked against the Key Registry at the point the Meta is read, so a typo fails at resolution with the file named rather than surviving to a mid-play undefined-variable error. The check lives in the resolver rather than in `PlaybookMeta::load`, because the Meta's other consumers — Backup Recipes, DNS subdomains, Unit Ownership — have no business holding a Key Registry to read a section they never touch.

Three fences hold the shape: every roster role has a Meta; every playbook has a Meta, because a playbook without one would validate nothing at all and say nothing about it; and every `required_keys` name across every Meta exists in the Key Registry, which was true before this change only by luck.

`Config` keeps sole authority over constructing a `Preflight` (`preflight_with_keys`), so the capability that unlocks `AnsibleRunner` still cannot be forged; only the resolution moved out, to the layer that can see the roster.

## Why

The two authorities had already disagreed, in both directions, and neither could be trusted to be the drift-free one.

`config.rs` doc-commented its match "the Key Registry — the single authoritative source of which config keys each playbook needs", while the Metas' `required_keys:` sat unread on the deploy path — declared data with no consumer, which is indistinguishable from a comment. `auberge ansible run gokapi` validated `admin_user_name` and `domain` from a `_ =>` fallback arm and nothing gokapi-specific, despite `gokapi.meta.yml` having declared the admin credentials all along.

A per-key grep of all 62 Key Registry names against the roles they belong to, taking a role's own `assert` or an unguarded reference with no default as proof of requirement, found the drift was not a handful of keys but 27, across ten Apps — baikal's admin password, all five of yourls' secrets, paperless' four asserted keys plus `admin_user_email`, grimmory's asserted four, bichon's two, and six `<app>_subdomain` keys with no default anywhere. Every one of them failed mid-play before, after the deploy had already begun changing the Host. Fail Fast is a stated principle of this repo; a preflight that validates three keys and lets 27 through is not one.

The audit also overturned the assumption the work started from. `colporteur_subdomain` looked like the clear over-enforcement — the hardcoded table demanded it, `colporteur.meta.yml` declared nothing, and the Meta carries `subdomain: colporteur`. But that field is a CLI-side default for DNS record management; nothing injects it into Ansible, and the role has no default of its own. Dropping the key would have moved a working deploy's failure from Preflight to an undefined variable. "Meta wins" is right as a rule for where the fact lives, not as a licence to treat the current Meta as correct: the grep decides, and here it added the key to the Meta instead of deleting it from the enforcement. One symmetric case ran the other way — `immich.meta.yml`'s missing `admin_user_name` is correct, because the immich role never reads it.

Deriving the requirements instead of declaring them was considered: the audit script is most of a derivation already. It was rejected as a runtime authority for the reason ADR-0042 rejected deriving unit ownership — derivation is blind in exactly the interesting places. `hostname` inside `<hostname>{{ radio_domain }}</hostname>` is an XML tag, not a variable reference; `actual_tailscale_ip` is a Key Registry name a role sets by `set_fact` from the Tailscale API and only accepts as an override; `baikal_busy_icloud_*` is optional behind `is defined`. A scanner that cannot tell these apart either demands keys nobody needs or misses keys everybody does. The scan's value is as an audit run by a person against a declaration, which is what it was here.
