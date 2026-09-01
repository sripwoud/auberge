# ADR-0075: The agent tier deploys as one guarded composition, and `deploy` reaches it

## Status

Accepted, 2026-09-01. Decided in #743, under the ruche epic (#747, [ADR-0054](./0054-agent-workloads-run-on-a-dedicated-disposable-host.md)). Completes the delivery half of [ADR-0071](./0071-a-tailnet-only-apps-parent-domain-is-per-app.md), which left the deploy-time resolution check "waiting on a composition that runs it".

## Decision

**`ruche.yml` is the agent tier's composition — a flat roster of `github_identity`, `opencode`, `aoe`, every entry `when: "'agent' in group_names"`.** The Host carries `tags = ["agent"]` in `hosts.toml`, which `ansible_runner` already emits as an `all.children.agent` group (#511, PR #514).

**A flat roster, not a meta role.** `parse_roster` reads a playbook's `roles:` list and nothing else, so a composition expressed as `dependencies:` in a `meta/main.yml` is invisible to every consumer that reads it — Preflight's key union, `units_for_run`'s failure report, `run_enters_role`'s pre-auth gate. `vibecoder` was the meta-role form and each of those consequences bit: nothing bound `bash_user_name` for it and it died mid-play, its roster was the one bare-string list in the tree, and its keys reached no Preflight.

**The guard goes on each App's own playbook too**, not on an `apps.yml` entry. `aoe.yml` and `opencode.yml` carry it, so no route — composition, standalone, or a hand-typed `--playbook` — reaches an unattended runtime on a Host that did not ask for one.

**The deploy-time DNS check reads a tagless run's roster.** It was gated on `run.is_apps()` and keyed on `run.tags`, and a composition carries neither — so the run ADR-0071 was waiting for would have been the one run that never checked. `published_apps` now returns a run's tags where it has them and its roster where it does not, the same expansion `units_for_run` already does for an untagged run. Substrate playbooks are excluded by name: they have rosters too, and neither publishes an App's name. An untagged `apps.yml` run is excluded as well — its roster holds guarded roles whose names belong to another Host.

**`auberge deploy` takes a standalone playbook name.** It validated against the `apps.yml` roster and nothing else, so `aoe`, `opencode` and `memsearch` were reachable only through `auberge ansible run -t <name>`. A name outside the roster now resolves to its playbook and runs after Substrate; a name the roster holds keeps going through the roster. `bootstrap` and `remove-radicale` are declared off the path in `NOT_DEPLOYABLE`, each with the reason it is a lifecycle operation rather than a convergence.

**A standalone run pulls `infrastructure.yml` in**, for the same reason an apps.yml tag does. The dashboard is unreachable until a Caddy holding a certificate for the agent tier's zone is in front of it ([ADR-0072](./0072-the-agent-tiers-caddy-answers-for-its-own-zone.md)), and that Caddy is configured in the infrastructure play or nowhere.

**A playbook whose every roster entry is guarded declares its roles' keys in its own Meta.** [ADR-0045](./0045-required-keys-are-declared-in-playbook-meta.md) has an untagged run select only the unguarded entries; where _everything_ is guarded that resolves to nothing, and Preflight passes on a config answering none of the tier's keys. `ruche.meta.yml` carries the union, held by `an_all_guarded_playbook_declares_the_keys_of_the_roles_it_guards` — which reads a role's own Meta where it has one and scans the role's YAML for Key Registry names where it does not, because `github_identity` has no Meta and reading only Metas would have passed over its three keys seeing an empty list.

**A Composition declares no `units:`, no `subdomain:` and no `backup:`.** The units belong to the Apps on the roster — `aoe.meta.yml` owns the tier's only one, and a second declaration would have a failed deploy name it twice — and `essaim` is aoe's name, which a second Meta claiming it would put in Blocky's `customDNS` map twice. Asserted in `test_ruche_meta_declares_no_backup_recipe` rather than merely omitted.

**The substrate roles are not in the composition.** `bash`, `fail2ban`, `kernel_hardening` and `tailscale` arrive from the hardening and infrastructure plays that every `deploy` prepends. `ufw` is bootstrap's, and must stay there: its second task is `ufw reset`, which _disables_ the firewall, and only the `ssh` role re-enables it after the port transition it validates. A composition re-running `ufw` alone would end every deploy of the agent Host with the firewall down.

## Why

The guard is the difference between operator discipline and a mechanism. A standalone playbook confines by nobody typing the wrong `--host`; nothing stopped `--playbook ruche.yml --host auberge` from installing an unattended YOLO runtime beside paperless and mail. The guard turns that into a no-op instead of an incident, and it is the `hermes` precedent rather than a new idea — `apps.yml` and `hermes.yml` already confine a single-Host role exactly this way.

Widening `deploy` is the smaller half of the same argument. Two verbs disagreed about which names exist, and the one an operator reaches for first was the one that knew fewer — so the documented way to bring up the agent tier was the surgical verb, which skips hardening, skips Substrate and skips the DNS check. Every one of those is something the agent Host needs and nothing else was going to supply.

The all-guarded Meta rule is the failure that reads as success. Preflight's whole promise is "fail before the first task", and on `ruche.yml` it would have passed a config missing `aoe_passphrase` and `opencode_openrouter_api_key`, installed `gh` and OpenCode, and then died on an undefined variable with the box half-converged. ADR-0045's guard exemption is right for `apps.yml`, which runs against every Host; it is exactly wrong for a playbook that exists only for the Host its guard names, where demanding the keys unconditionally costs nothing.

## Trade-off

- **`ruche.meta.yml` restates the keys its roles' Metas already declare.** A duplication, and the kind that goes stale silently — which is why it is fenced rather than reviewed. The alternative, deriving Preflight's demand from guarded roles, is the runtime-authority derivation ADR-0045 rejected.
- **The DNS check now runs on more kinds of run than before.** A standalone playbook that publishes a name gets verified where it previously was not — correct, and a behaviour change: `auberge deploy calibre` can now fail on a DNS mismatch it used to deploy straight through.
- **`deploy` now takes names that are not Apps.** `ruche` is a Host's composition, not something with a subdomain or a Backup Recipe, and `auberge deploy ruche -H ruche` reads oddly for it. Accepted: the alternative is an operator typing three App names in the right order.
- **`NOT_DEPLOYABLE` is a list someone must maintain.** Held in both directions by `tests/deployable_playbooks.rs`, so a stale entry and an unclassified new playbook both fail the build.

## Alternatives considered

- **A `ruche` meta role in `apps.yml`, deps `[github_identity, opencode, aoe]`** (the issue's own wording). Rejected: `parse_roster` cannot see a `meta/main.yml` dependency, so the `ruche` tag would resolve to no App — no DNS check for `essaim`, no unit in the failure report, and aoe's and opencode's keys restated in a Meta with no fence able to compare them against the roles they came from. It is the shape `vibecoder` died in.
- **`aoe` and `opencode` as guarded `apps.yml` entries**, so the roster route would run the DNS check. Built, then reverted: `selected_roles` ignores a guard whenever tags are named, and `auberge deploy --all` names every roster role — so every Host in the fleet would have had its Preflight demand `agents_domain`, `aoe_passphrase`, `aoe_tailscale_ip` and `opencode_openrouter_api_key`, and aoe is the first guarded roster role carrying a `subdomain:`, so the `essaim` check would have fired on Hosts that publish no such name. `--all` is the documented CI/CD path. **The guard that confines a role at play time does not confine the Preflight that runs ahead of it** — that asymmetry is what this alternative hides.
- **Leave `deploy` alone and document `auberge ansible run -t ruche`.** Rejected: that path runs neither hardening nor infrastructure, so the documented way to build the Host would be the one that omits the Caddy the dashboard needs and the DNS check that proves it resolved.
- **Give `ruche.meta.yml` `subdomain: essaim` and `domain_key: agents_domain`** so `deploy ruche` runs the DNS check itself. Rejected: two Metas would claim one name, and Blocky's `customDNS` map is built from those Metas — the same name twice, published from one Host.
- **Make `deploy` resolve a _tag_ rather than a role name**, so `ruche` could be a tag on the aoe and opencode entries. Rejected: every App-keyed lookup in the CLI reads `<tag>.meta.yml`, so a tag naming no App silently skips the DNS check and the unit readout — green, and having verified nothing.
- **Leave the DNS check gated on `is_apps()`** and accept that the composition does not verify `essaim`. Rejected: it makes the command the docs hand an operator the one command that cannot tell them whether the dashboard resolved, which is the failure ADR-0071 was written about.
- **Put `ufw` in the composition**, as `vibecoder` did. Rejected on the `ufw reset` reading above: it would disable the firewall on every deploy of the one Host the fleet assumes is compromisable.
- **A `github_identity` entry in `apps.yml` too.** Rejected: it is Host identity, not an App, and it has no Meta, no unit and no name to publish. It stays on the composition, which is also the rotation path `docs/configuration/fleet-github-identity.md` names.
