# ADR-0041: A removed unit clears its own failed state

## Status

Accepted, 2026-08-25. **Applies ADR-0028's declared regime to package contents** — the fourth fact about a unit the repo cannot compute, after restart edges (ADR-0028), directory writability (ADR-0035) and clean-shutdown exit statuses (ADR-0038).

## Decision

`tests/removed_unit_failed_state.rs` computes every site in `ansible/roles` and `ansible/playbooks` that makes a systemd unit disappear, by two routes:

- an `ansible.builtin.file` with `state: absent` whose path is a plain unit file in a directory systemd loads units from — drop-ins excluded, since a `<unit>.service.d/` never matches a unit suffix;
- an `ansible.builtin.apt` with `state: absent`, whose packages are resolved to units through `PURGED_PACKAGES`.

Both readers follow the modules' parameter aliases — `path`/`dest`/`name` and `name`/`package`/`pkg` — because ansible accepts all of them and the repo writes unit files under `dest:` everywhere it installs one. Reading only the canonical spelling made a removal written the way the rest of the tree writes it invisible to the scan; that was caught in review, by planting one.

Which units a Debian package ships is not in this repo, so that mapping is declared. Six entries today, each read off the package with `dpkg -c` rather than inferred from its name: `apache2` ships `apache2.service` and `apache-htcacheclean.service`; `radicale` ships one unit; `apache2-bin`, `apache2-data`, `apache2-utils` and `libapache2-mod-php8.4` ship none. `@` templates are deliberately absent from the table — a template is never itself instantiated, so it has no state to latch, and `reset-failed` on a bare template name addresses nothing.

The pairing is asserted in both directions:

- every removed unit must be named by a `systemctl reset-failed` **in the file that removes it**, because the reset has to run wherever the removal does;
- every package the tree purges must appear in `PURGED_PACKAGES`, so a newly-purged package is classified before it can ship;
- a declared package nothing purges any more fails, so the table cannot outlive the task that needed it;
- every reset names **exactly one** unit, and no glob (below).

The scan's reach is pinned as a set, `<file>::<unit>`, the way ADR-0035 and ADR-0038 pin theirs and for the same reason: a count stays green when one removal replaces another and cannot name which moved.

## Why

`failed` is a terminal state. systemd leaves it on `reset-failed`, on a successful `start`, or when the unit is garbage-collected — and on nothing else. Both halves of that were measured against a throwaway unit rather than assumed:

- `systemctl stop` on a failed unit returns 0 and leaves `ActiveState=failed`. The stop the role already ran cleared nothing.
- deleting the unit file and reloading strands the state as `not-found failed failed` — the exact signature auberge carried.

So the fleet's remove-a-service sequence — stop, disable, purge — produces a permanent phantom whenever the unit was failed at the time. auberge carried `apache2.service not-found failed` from bootstrap on 2026-08-21 until it was cleared by hand, on a host with no apache2 installed: php pulls apache2 in as a dependency, its postinst starts it against a port caddy already holds, and the purge that follows takes the unit file away and leaves the failure behind.

`systemctl --failed` is the fleet's first-look health check, and it is only worth reading if everything in it is real. One permanent entry is enough to stop anyone reading it — the same cost ADR-0038 priced for grimmory's nightly latch, reached by a different route.

#646 closes the apache2 instance from the other end, pinning the package out of reach so php never pulls it in. That is the better fix for that one package and it is not this one: it leaves the boxes that already latched, and it says nothing about the next service the fleet removes. This ADR is about the removal path itself, which is why the fence keys on removal sites rather than on apache2.

Fenced rather than fixed twice, because a one-time audit is what the first fix already was, and it missed two things this fence names. `apache2` ships **four** units, not the one the original task reset — `apache-htcacheclean.service` would have latched by exactly the same mechanism. And `remove-radicale.yml` is the tree's other stop-and-remove site: it stops `radicale`, deletes its unit file, purges the package, and cleared nothing. Two misses in one audit is the argument for the fence.

One unit per invocation, which looks like verbosity and is not. There is no ansible module for `reset-failed`, and ansible-lint's `command-instead-of-module` allowlist admits `reset-failed` while rejecting `is-failed` — so at this point in the play the state cannot be read to guard the task, and rc is the only signal there is. The qualifier matters: cockpit does guard its own `reset-failed` on `cockpit_wait_online.status.ActiveState == 'failed'`, read off a registered `systemd_service`. That works because cockpit _masks_ the unit rather than removing it, so the unit is still loadable and has a status to register. Past a purge there is no unit for `systemd_service` to report on, and the idiom does not transfer. Past the purge the unit file is gone, so the unit is loaded only if residual state holds it there: rc 0 means a latch was cleared, rc 1 (`not loaded`) is the steady state, and `changed_when: <result>.rc == 0` is exactly right. Both shorter forms destroy that, as measured:

- `reset-failed a b` returns 1 when **any** argument is not loaded, even in the run where it cleared another — a real change reporting as unchanged;
- `reset-failed 'apache2*'` returns 0 whether it cleared a latch or matched nothing — every deploy reporting as changed.

`changed_when` evaluates per iteration inside a `loop:`, confirmed against a real playbook run, so looping keeps one rc per unit.

## Considered alternatives

- **Clear the latch by hand and close the ticket.** Available — it is what unblocked auberge. Rejected as the whole answer: it fixes one unit on one host, leaves the other unit apache2 ships and the other site that removes a service, and catches nothing later. #646's pin closes the apache2 recreation path; neither it nor a hand-clearing closes the mechanism.
- **Delete `remove-radicale.yml` as spent.** The radicale-to-baikal migration is long done and the playbook has not been touched since. Rejected as out of scope and more invasive than the bug: it is a first-class registered playbook with a `.meta.yml`, a `playbook_meta.rs` test, and named exclusions in `dns.rs`, `dependency_resolver.rs` and `backup/recipe.rs`. Retiring it is an inventory decision, not a latch fix.
- **Stop apache2 from ever being installed** — pin it out so php cannot pull it in. Not an alternative: it landed separately as #646 (`Pin-Priority: -1` on `apache2`, `apache2-bin`, `apache2-data` and `libapache2-mod-php*`), and the two halves close different ends of the same bug. The pin stops a fresh box ever creating the latch; it does nothing for the boxes that already carry one, nothing for the other packages the fleet removes, and nothing for the mechanism — which is a property of removing any service, not an apache2 quirk. Prevention and remediation are both required, and only the second generalises.
- **A `reset-failed` on every unit at the end of each deploy.** Rejected: it would clear real failures too, which is precisely the signal being protected.
- **Compute package contents instead of declaring them.** Rejected as unavailable — `apt-file` is not installed on the fleet, and reading a `.deb` at test time would put the Debian archive in the build's dependency path. The declaration's limit is recorded below.
- **Guard the reset on the unit's state, the way cockpit does.** Rejected because it is unavailable here, not because it is worse: cockpit's `when:` reads a status off a unit it masked and therefore left loadable, and after a purge there is no unit to read. The reset has to be safe to run against nothing, which is what makes rc the guard.
- **Assert the reset anywhere in the tree, not in the removing file.** Rejected: a reset in some other role does not run when this removal does, and file scope is what makes the assertion mean anything.

## Consequences

**Positive:**

- The build now fails on: a unit removed with nothing clearing its state, a purged package `PURGED_PACKAGES` does not classify, a declared package nothing purges, a reset naming more than one unit or a glob, a package name or reset target that does not resolve, and a removal site the reach set does not list or lists and no longer finds. Each failure names the file, the task, the unit, and what to add.
- Every failure mode above was exercised by mutation before the fence was trusted: six planted defects, six catches, both detection routes among them.
- `systemctl --failed` on auberge is empty and stays that way through a rebootstrap, rather than only until the next one.

**Negative:**

- Package contents are declared, not measured. `PURGED_PACKAGES` is right as of the versions read; a Debian revision that adds a unit to a package listed as shipping none would make the declaration wrong with nothing to catch it. The defining limit of the declared regime, the same one ADR-0038 records for runtimes.
- The scan reads literal package names, literal unit file names, and literal `loop:` items. Each of those is a hard stop rather than a silent miss — but only where the model gets far enough to see it. A removal whose _directory_ is var-driven fails the directory test and leaves the domain unseen, the same blind spot ADR-0038 records for units installed somewhere its model does not look. The fleet writes every unit path literally today; the file-name hard stop closes the case that review proved could slip through, and this one is recorded rather than closed.
- The reach set binds to file and unit names, so adding, removing or renaming a removal site fails this test until the list moves with it. Loud and cheap, the trade ADR-0028 already accepted.
- Nothing here proves the reset _runs_ — only that it is written next to the removal. The rc semantics it depends on were measured on a live latch and on a clean host; the ansible task's own behaviour is not exercised by any test in this repo.

## References

- Issue #636 — the latched phantom, and the acceptance criterion asking for the same check on every other package the role stops-and-removes.
- Issue #635 / ADR-0038 — the same false positive reached by a different route, and the audit that turned this one up.
- ADR-0028 — the declared regime: computed key, per-entry why, equality in both directions.
- ADR-0035 — the precedent for pinning a computed domain by set equality rather than by count.
