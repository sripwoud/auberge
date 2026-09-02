# ADR-0077: The deploy menu offers every target, but `[all apps]` stays the roster

## Status

Accepted, 2026-09-01. Closes #803, a gap in [ADR-0075](./0075-the-agent-tier-deploys-as-one-guarded-composition.md)'s widening of `deploy`. Completes it rather than supersedes it: the named-arg path that ADR decided is unchanged, and only the path an operator reaches with no arguments at all is brought into line with it.

## Decision

**The interactive selector offers every name `deploy` accepts.** `select_apps` is handed the `apps.yml` roster _and_ `deployable_playbooks()`, the same two lists `validate_apps` already checks a typed name against. Order is the roster in `apps.yml`'s declaration order, then the standalone playbooks the roster does not hold, sorted.

**A name that is both is offered once, bare.** calibre, gokapi, hermes and immich are each an `apps.yml` role and a standalone playbook; `split_routes` sends them through the roster whichever way they were selected, so two entries would print two ways to deploy one thing.

**A standalone playbook is marked `(playbook)` in the menu, and the marker is stripped on selection.** The names either side of it deploy differently — one is a tag into `apps.yml`, the other a whole Playbook run after Substrate — and the plan that prints afterwards is the first place that difference currently shows.

**`[all]` is renamed `[all apps]` and still expands to the roster alone.** It is the same set `--all` deploys. Selecting it alongside individual entries deploys the union, deduplicated; it previously discarded the other picks.

## Why

The two entry paths disagreed about which names exist, which is the same defect ADR-0075 fixed one level up — it just fixed it in `validate_apps` and left `select_apps` reading `get_app_names()` alone. So `auberge deploy ruche -H ruche` worked and `auberge deploy` never mentioned `ruche`, which makes the documented feature reachable only by someone who already knew it existed. #803 was filed from exactly that position: bare `deploy`, run the week `ruche` shipped in 0.17.0, listing everything except the thing that had just shipped.

`docs/cli-reference/deploy.md` promised the menu listed them, so the docs were the accurate description of the intent and the code was the bug.

### Why `[all apps]` does not reach them

The issue proposed it, and it is the reading the old `[all]` label invites. It is wrong on two independent counts, and the first is the one that generalises.

**Preflight is built for the whole plan, before the first task.** `run_deploy` maps `preflight_for` over every run and collects with `?`, so an unsatisfied key on _any_ run in the plan fails the deploy before ansible starts. Putting `ruche` into `[all apps]` therefore makes `agents_domain`, `aoe_passphrase` and `opencode_openrouter_api_key` mandatory on every Host anyone deploys everything to — including the ones that will never run an agent. This is not a new discovery: ADR-0075 built the `aoe`-and-`opencode`-as-guarded-`apps.yml`-entries alternative, reverted it, and recorded the reason as **the guard that confines a role at play time does not confine the Preflight that runs ahead of it.** The `when: "'agent' in group_names"` guards make the _run_ a no-op; they do nothing about the key demand in front of it.

**And the guards are not uniform anyway.** `memsearch.yml` is `hosts: all` with no `when:` on either roster entry. It is the one deployable standalone playbook that is not confined to the agent group, so a sweep would not merely cost a wasted no-op run — it would install syncthing and the agent memory store on whatever Host `[all apps]` was pointed at. Left as-is: `memsearch` is box-global by design ([ADR-0064](./0064-agent-memory-pools-in-one-directory.md)) and reaching it deliberately is the intended path. But it means the safety of a sweep cannot be argued from the guards.

So the menu widens and the sweep does not. A standalone playbook is opt-in by name, in both the menu and the arguments — which is also what keeps `[all apps]` and `--all` describing one set, rather than two that drift.

### Why the label had to change with it

`[all]` sitting above a list it covers two thirds of is a menu that lies, and the lie is in the dangerous direction: an operator picking it to "deploy everything on this box" gets the apps and silently not the tier. `[all apps]` names the set, and the `(playbook)` markers make it checkable by eye — everything unmarked is what `[all apps]` takes.

`playbook` rather than `composition`: CONTEXT.md reserves **Composition** for a Playbook whose roster is several Apps, and says `ruche.yml` is the only one. `aoe`, `opencode` and `memsearch` are plain standalone Playbooks, so marking all four `(composition)` would put a word in the UI that the domain model spends a paragraph narrowing.

### Why the fence is agreement, not a list

`test_every_menu_entry_is_a_name_the_named_arg_path_accepts` builds the menu, resolves every entry as if selected, and runs the result through `validate_apps`. It asserts the two paths agree about which names exist, which is the defect itself, rather than asserting either path's contents — a literal list of `["ruche", "aoe", …]` would need editing the day a playbook lands and would pass vacuously if `menu_items` started returning fewer.

All five mutations were run: dropping the standalone group, keeping a both-lists name twice, leaving the marker on, dropping the dedup, and expanding `[all apps]` to the union. Each fails the test named for it, and the last fails `test_all_reaches_no_playbook_in_the_real_tree`, which reads the tree and asserts it has something off-roster to be non-vacuous about.

## What it costs

- **The menu is longer by four entries and mixes two kinds of thing.** The marker mitigates it; it does not remove the fact that an operator now scrolls past `ruche` on a Host that will never run it.
- **`(playbook)` is presentation encoded into the value the selector returns**, and stripped a few lines later. A round-trip through a decorated string is a seam that can break silently, so it is asserted in both directions rather than reviewed. `select_multi` takes `&[String]` and returns the labels, so _a_ round-trip is forced; a `(label, name)` pair threaded through `select_apps` would have made it a lookup rather than a parse. Not taken — the parse is one `strip_suffix` with a fence either side of it — but it is the shape to reach for if a second marker ever appears.
- **`deployable_playbooks()` is now computed on every `run_deploy`**, including the `--all` path that does not consult it. One directory read against a plan that is about to run ansible; taken to keep the two branches reading one list rather than each building its own.
- **`[all apps]` is a rename of a string operators have muscle memory for.** Accepted: the old string's meaning is what changed underneath it, so keeping it would have been the worse break.
- **`memsearch.yml`'s missing group guard is documented here and not fixed.** It is load-bearing for this decision — the argument against a sweep leans on it — which means a later commit adding the guard would quietly remove one of the two reasons. The Preflight reason stands alone, and is the one to check first if this is revisited.

## Alternatives considered

- **Offer the union and let `[all apps]` take all of it,** as #803 suggests. Rejected on the Preflight reading above; it is ADR-0075's rejected alternative arriving through a different door.
- **Offer the union, keep `[all]` meaning the roster, change no label.** Rejected: it is the current bug's cause re-created one layer up — a control whose name describes a set it no longer selects.
- **Leave the names bare, with no `(playbook)` marker.** Simpler, and the failure mode is benign (an operator picks `[all apps]` and a composition does not run). Rejected because discoverability _is_ #803: a bare `ruche` in an alphabetical list of apps is findable, but it does not tell the operator that selecting it runs a whole playbook after Substrate rather than one role.
- **A separate `auberge deploy --list` or a second menu for playbooks.** Rejected: two menus is two places for the two paths to drift apart again, which is the defect.
- **Sort the whole union alphabetically,** as the issue suggests. Rejected: the roster's order is `apps.yml`'s own and operators read it that way today. Sorting only the appended group keeps the existing list where it was and makes the new entries a visible block.
- **Add the missing `when: "'agent' in group_names"` guard to `memsearch.yml`** so a sweep would be safe. Out of scope, and it would not make a sweep safe — Preflight runs ahead of the guard, which is the whole point above.

## References

- Issue #803 — the defect. #801 / [ADR-0075](./0075-the-agent-tier-deploys-as-one-guarded-composition.md) — the widening this completes, and the source of the Preflight argument.
- `src/commands/deploy.rs` — `menu_targets`, `menu_items`, `resolve_menu_selection`, `select_apps`.
- `tests/deployable_playbooks.rs` — holds the deployable set against the tree; unchanged, and the source the menu reads through.
- CONTEXT.md — **Playbook**, **Composition**, **App**.
