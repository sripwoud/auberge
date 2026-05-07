# Auberge

Self-hosted homelab provisioning: a Rust CLI that runs Ansible playbooks against user-owned hosts and backs up the apps it deploys.

## Language

**Playbook**:
An Ansible playbook (`ansible/playbooks/<name>.yml`) that deploys exactly one app or one piece of infrastructure to a Host.
_Avoid_: Role, recipe (Ansible-internal), task (Ansible-internal)

**Playbook Meta**:
A sibling YAML file (`ansible/playbooks/<name>.meta.yml`) declaring the Playbook's contract with auberge — its `required_keys` from the Key Registry and, optionally, a `backup` section holding a Backup Recipe.
_Avoid_: Manifest, descriptor, schema

**Key Registry**:
A single file (`ansible/keys.yml`) listing every config key auberge knows about, with per-key metadata (secret, doc string). The vocabulary of `Config`.
_Avoid_: Schema, dictionary, catalog

**Config**:
The merged user-supplied settings (`config.toml`) parsed against the Key Registry. There is no static `config.example.toml`; users run `auberge config init` to generate a starter file from the registry.
_Avoid_: Settings, options, user config, env

**Preflight**:
A capability type carrying a validated `Config` + a Playbook Meta. The only way to construct one is via `Config::preflight_for(playbook)`, which validates required keys. `AnsibleRunner::run` accepts only a `Preflight`, making it impossible to invoke ansible with unvalidated config.
_Avoid_: Plan, request, prepared run

**Host**:
A target machine in the Inventory (name, user, IP, SSH key). Playbooks run against one Host at a time.
_Avoid_: Server, node, target, machine

**Inventory**:
The version-controlled list of Hosts in `ansible/inventory.yml`. (Distinct from `hosts.toml`, which is user-local and used only by backup operations — see ADR.)
_Avoid_: Hostlist, fleet

**App**:
An application deployed by a Playbook (e.g. paperless, navidrome, baikal). An App has a Backup Recipe iff its Playbook Meta includes a `backup:` section.
_Avoid_: Service, package, workload

**Tailnet-only App**:
An App whose Playbook Meta declares `tailnet_only: true` (and a `subdomain:` field as the canonical default for FQDN composition). Caddy binds only to the host's Tailscale interface; the App's hostname is published only via Blocky's `customDNS` map — derived at deploy time from the meta files of all `tailnet_only` Apps, with the operator's `<app>_subdomain` in `config.toml` taking precedence over `meta.subdomain` when defined — and does _not_ appear in public DNS. Reachable only by clients on the user's tailnet, via Blocky as resolver. Headscale's `dns.nameservers.split` routes `*.{{ domain }}` queries to Blocky so every tailnet client uses Blocky for the user's domain without manual client-side DoT setup.
_Avoid_: Private app, internal app, vpn-only app

**Public App**:
An App without `tailnet_only`. Caddy serves on the host's public address; DNS publication is a Cloudflare A record pointing at `ansible_host` (via the `dns_record` role).
_Avoid_: External app, world-facing app

**Substrate App**:
An App whose deploy state must be present and correct before another App's deploy can verify reachability — currently Caddy (HTTPS for every App), Headscale (login server for Tailscale on first deploy), and Blocky (DNS Publication for every Tailnet-only App). Substrate Apps are declared in `ansible/playbooks/infrastructure.yml` and run on every `auberge deploy`, regardless of `--tags`. Orthogonal to the Public App / Tailnet-only App axis: a Substrate App may itself have a subdomain (e.g. `hs`, `blocky`) but is placed by its dependency role, not by `tailnet_only`. See ADR-0005.
_Avoid_: Infrastructure component, shared service, platform service

**DNS Publication**:
The act of making an App's hostname resolvable, performed during deploy. For Public Apps it is a Cloudflare A record; for Tailnet-only Apps it is a Blocky `customDNS` entry. Either is part of `auberge deploy`'s success criterion — a deploy that completes without a working DNS answer is treated as a failure.
_Avoid_: DNS setup, record creation, A-record provisioning

**Backup Recipe**:
The declarative `backup:` section of a Playbook Meta describing how to back up the App: services to stop, paths to rsync, optional database dump, optional `post_restore_command`. Pure data — no imperative branching. Most Recipes capture an App's on-disk state directly; for Bichon the Recipe rsyncs an **Email Archive** instead, see ADR-0006.
_Avoid_: Backup config, backup plan, strategy

**Recipe Executor**:
The Rust module that executes one Backup Recipe against one Host: stop services → optional DB dump → rsync paths → optional DB restore → start services. Issues every command through the `SshSession` trait (the only test seam).
_Avoid_: Backup runner, recipe runner

**Backup Session**:
The Rust module that orchestrates multiple Recipe Executor invocations across a Host's Apps, plus restic push and prune. Owns cross-recipe concerns; per-recipe semantics live in the Recipe Executor.
_Avoid_: Backup job, backup workflow

**Email Archive**:
A Bichon-independent on-disk mirror of email messages, produced by an hourly systemd timer on the bichon Host that walks Bichon's REST API and writes one `.eml` per message under `/var/lib/bichon-archive/`, with a `.meta.json` sidecar capturing folder + tags. Distinct from a **Backup Recipe**: an Archive is consumable without Bichon (any MBOX/EML-aware client can read it), and is the _source_ the bichon Backup Recipe rsyncs — Bichon's encrypted internal store (`/opt/bichon/data`) is deliberately not backed up. See ADR-0006.
_Avoid_: backup (collides with Backup Recipe), dump, export.

**Upstream Mailbox**:
The third-party IMAP (or Gmail-API) server that Bichon syncs _from_ — e.g. the operator's Gmail, Fastmail, ProtonMail Bridge endpoint. Distinct from the **Email Archive** (Bichon-side, append-only) and from any **Backup Recipe** target. Operations on the Upstream Mailbox (e.g. expunging old mail to reclaim quota) are out-of-scope for `auberge deploy` and `auberge backup`; any future tooling that touches it must treat it as authoritative-but-untrusted.
_Avoid_: IMAP server, mail provider, source mailbox.

**Synced Folder**:
A folder on an **Upstream Mailbox** that Bichon ingests into the **Email Archive**. The set is computed at **Account Reconcile** time as `(remote folder list) − (exclusion set)`, where the default exclusion set is `{Spam, Trash}` — folders whose user-meaning is "not real mail" or "I'm done with this", which an Archive must not invert. The result is written into Bichon's per-account `sync_folders` field.
_Avoid_: All folders, every folder, full mailbox, watched folder.

**Account Reconcile**:
The auberge-driven step that reads each Bichon account's live folder list (`GET /api/v1/list-mailboxes/<id>?remote=true`), computes the **Synced Folder** set, and PATCHes Bichon's per-account `sync_folders`. Account _creation_ (IMAP host, credentials, OAuth2 consent) remains UI-driven; reconcile is the only state auberge writes to Bichon. Folder identity is matched primarily by RFC 6154 SPECIAL-USE attributes (language-portable, hierarchy-portable), with case-insensitive name matching as fallback for legacy IMAP servers that don't advertise SPECIAL-USE.
_Avoid_: Account sync, account update, account migration.

**Progress**:
The trait that runners (`AnsibleRunner`, `Recipe Executor`, `Backup Session`) emit events through. `TerminalProgress` is the production impl; tests use a `MockProgress`. Keeps runners free of terminal-output coupling.
_Avoid_: Logger, reporter

## Relationships

- A **Playbook** has exactly one **Playbook Meta** sibling.
- A **Playbook Meta** declares zero or more keys from the **Key Registry**.
- A **Playbook Meta** declares zero or one **Backup Recipe**.
- A **Preflight** binds one **Playbook Meta** to a validated **Config**.
- The **Recipe Executor** consumes one **Backup Recipe**; the **Backup Session** consumes many.
- All runners report through **Progress**; none touch terminal output directly.
- An **App** is either a **Public App** or a **Tailnet-only App**, determined by the `tailnet_only` flag in its **Playbook Meta**. **DNS Publication** is dispatched accordingly.

## Example dialogue

> **Maintainer:** "Paperless needs a new env var. Where do I add it?"
> **Reviewer:** "Add it to the **Key Registry** with `secret: true` if it's sensitive, then list its name in `paperless.meta.yml` under `required_keys`. The next `auberge ansible run paperless` will fail-fast if the user hasn't set it."

> **Maintainer:** "Why doesn't the **Recipe Executor** know about restic?"
> **Reviewer:** "Restic push and prune are cross-recipe — they happen once per **Backup Session**, not once per **Backup Recipe**. The split is the whole reason those two modules exist."

## Flagged ambiguities

- "Backup runner" was used loosely for both per-recipe and multi-recipe execution. Resolved: use **Recipe Executor** (one recipe) and **Backup Session** (many recipes) — never "runner" without qualification.
- "Spec" was used early in the design conversation for what became **Playbook Meta**. Resolved: avoid "spec" — it conflicts with Rust's `cargo spec` and reads ambiguous next to "schema."

## Stdout discipline

**stdout is data; chrome goes to stderr.**

This rule follows [clig.dev](https://clig.dev/#output): programs should print only their primary data output to stdout so that output can be piped, redirected, or parsed (including as JSON) without noise. Status messages, hints, confirmations, and any other "chrome" must go to stderr.

In practice:

- `println!` and `print!` are allowed **only** in modules that emit the command's primary data output (e.g. `config_cmd`, `dns`, `headscale`, `host`, `select`, `backup`, and `output::print_table`).
- All other informational messages — "Cancelled.", spinner updates, success banners, hints, interactive prompts — must use `eprintln!`/`eprint!`, `output::info`, `output::success`, or `output::warn`, all of which write to stderr.
- Interactive prompts that read from stdin should `eprint!` the prompt and `io::stderr().flush()`, so the prompt is visible on the TTY even when the caller pipes stdout.

A CI step in `.github/workflows/master.yml` enforces this by failing if `println!`/`print!` appears in any source file outside the approved allowlist. The check is per-file, so modules on the allowlist are on the honor system: chrome that lives in an allowlisted file is not caught.
