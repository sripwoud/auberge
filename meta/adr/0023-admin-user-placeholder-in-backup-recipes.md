# ADR-0023: Backup Recipes reference the admin user via a `{admin_user}` placeholder

## Status

Accepted, 2026-08-20.

## Decision

A Backup Recipe may write `{admin_user}` in any of its string fields. `load_app_recipe` substitutes it with the target Host's `user` (from `hosts.toml`) before the recipe reaches any consumer, so the Recipe Executor, the restore pre-flight, and every display path only ever see resolved values.

First consumer: syncthing's Recipe (#512). Syncthing runs as the admin user through the packaged template unit `syncthing@<user>`, and its device identity lives in that user's home:

- unit: `syncthing@{admin_user}`
- paths: `config.xml`, `cert.pem`, `key.pem` under `/home/{admin_user}/.local/state/syncthing`
- owner: `{admin_user}:{admin_user}`

The Recipe enumerates the three identity files instead of the state directory: `index-v*.db*` is a rebuildable cache — churn-heavy, restic-dedup-hostile, and worthless in a restore. Enumerating files _is_ the exclusion mechanism; Recipes have no `exclude:` field and don't need one for this.

## Why

Recipes are repo data (ADR-0001); the admin user is per-operator config. Syncthing is the first App whose unit name and paths depend on that config, so some late-binding is required. The Host's SSH `user` is the value `syncthing_user` defaults to at deploy (`admin_user_name | default(ansible_user)`), and backup operations are already keyed to a Host from `hosts.toml` — the substitution source is data the backup flow always has.

## Considered alternatives

- **Hardcode the username in the meta file.** Rejected: embeds one operator's config in repo data; wrong for every other operator and for any host whose admin user differs.
- **Jinja-style `{{ admin_user_name }}` rendered through a template engine.** Rejected: these files are consumed by Rust, never by Ansible. A template dependency for one variable is overkill, and Jinja syntax would suggest Ansible semantics (filters, defaults) that don't exist here.
- **Resolve inside the Recipe Executor instead of at load.** Rejected: recipes are loaded at five call sites and inspected outside the executor (pre-flight service checks, cross-host hints). Resolving at load makes an unresolved recipe unrepresentable downstream; a forgotten `resolve()` call cannot exist.
- **Back up the whole state directory to avoid file paths.** Rejected: captures the index cache (see Decision) and would need new `exclude:` recipe grammar to avoid it.

## Consequences

- On cross-host restore the placeholder resolves against the _target_ Host's user: identity files land in, and are owned by, the target's admin home. If source and target admin users differ, the restore fails fast on the mismatched archive layout rather than silently writing to the wrong home.
- Syncthing's Recipe is the first with single-file paths and a template-unit instance; the live paths were generalized for both (`rsync_to` no longer assumes a directory source; the pre-flight unit check maps `name@instance` to the `name@.service` template file, since `systemctl list-unit-files` does not match instances).
- Losing these three files silently breaks folder sharing with every peer (the device ID changes) — the 2026-08-20 OVH rescue had to salvage them by hand. With this Recipe they ride every `auberge backup`.

## References

- Issue #512 — syncthing has no playbook meta: identity is never backed up.
- ADR-0001 — Declarative Backup Recipes. The placeholder keeps recipes data; substitution is not branching.
