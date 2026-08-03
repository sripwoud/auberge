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

**Backup Verdict**:
The verdict `auberge backup verify` reaches about a Host's newest offsite restic snapshot — or, with `-a`, the newest snapshot _holding that App_, which a partial sync can leave behind an App-less newer push — as a fail-fast checklist: repository reachable → a snapshot exists → it contains an App → it is younger than a threshold, carried by the exit code (0 verified, 1 a check failed, 2 operational error). A pure function of `restic snapshots --json` plus one containment probe per candidate snapshot, so the whole decision is unit-tested without invoking restic. Asserts the "backup is current" half of ADR-0007's boundary; it says nothing about the **Upstream Mailbox**, which would be coverage-check and stays out of scope.
_Avoid_: Health check, audit, validation, integrity check (restic's own `check` verifies repository integrity — a different question).

**Internal Store**:
Bichon's own on-disk state (`/opt/bichon/data`) — a Tantivy index of envelopes plus a content-addressed blob store of raw messages. A **rebuildable cache, not a store of record**: when an **Upstream Mailbox** changes a folder's `UIDVALIDITY`, Bichon discards that folder's envelopes _and_ their blobs and refetches from the server, so any message no longer present upstream is lost from the Internal Store. Deliberately not backed up (ADR-0006); the **Email Archive** is the durable copy. Everything the web UI renders is served from here, which is why a purge costs searchability even when the Archive is intact.
_Avoid_: Bichon database, index, cache (understates that it holds the only searchable copy), data dir.

**Email Archive**:
A Bichon-independent on-disk mirror of email _bodies_, produced by an hourly systemd timer on the bichon Host that walks Bichon's REST API and writes one `.eml` per message under `/var/lib/bichon-archive/<account>/YYYY/MM/`, with a `.meta.json` sidecar recording the folder and the message's identity. Append-only in the strong sense for _observations_ — **what the archive records about a message is written once and never revisited** — so it faithfully preserves what is immutable (bodies, attachments) and captures folder at first sight; mutable metadata lives in the **Tag Snapshot** instead. A derived index key may be repaired in place, which is how `message_id` reached sidecars written before it existed (ADR-0013). Distinct from a **Backup Recipe**: an Archive is consumable without Bichon (any MBOX/EML-aware client can read it), and is the _source_ the bichon Backup Recipe rsyncs. Folder is carried in the sidecar rather than the path, because Bichon's own importer derives folder from the full relative directory path and would read the date partitions as part of the folder name. See ADR-0006, ADR-0012, ADR-0013.

**A body is published only if it is a message** — at least one header field line, then the RFC 5322 separator. Bichon answers `200` with a zero-length payload for an envelope whose blob store entry is empty, and `curl --fail` reads that as success, so one 0-byte `.eml` reached the corpus and the skip guard's `[ -f ]` guaranteed it was never retried. A refused payload is a counted failure: the cursor does not advance and the next run asks again. A body already archived that fails the same check is refetched by the envelope id its filename carries (ADR-0015).

**A message's identity is its RFC 5322 `Message-ID`**, canonicalized (unfolded, angle brackets stripped) from the archived `.eml` and recorded in the sidecar as `message_id`; a body with no such header is keyed `sha256:<hex>` over the `.eml`. The `<envelope-id>` in the filename is a **storage detail, not an identity** — Bichon mints a fresh envelope identifier on re-import, so the same message can occupy several `.eml` files. Anything counting messages must count distinct `message_id`; counting files over-reports, which is why `examples/bichon-expunge.sh`'s coverage gate refuses to.
_Avoid_: backup (collides with Backup Recipe), dump, export. Never call the `<envelope-id>` filename the message id.

**Archive Cursor**:
A per-account file (`/var/lib/bichon-archive-state/<account>.cursor`, `0600 bichon:bichon`) holding the largest message `Date:` (epoch ms) a failure-free archive run has seen; the next run asks Bichon only for envelopes dated after `cursor − overlap` (24h). A **message-date watermark, not an ingestion watermark**: the overlap absorbs clock skew and delivery lag, but both `Date:` and IMAP `INTERNALDATE` travel _with_ the message — `MOVE` preserves `INTERNALDATE` (RFC 6851) and no operation rewrites `Date:` — so mail entering a **Synced Folder** by move, bulk import, or with a backdated header sits below any such watermark forever: present in the **Internal Store**, invisible to the **Email Archive**, and unreported. The repair is a full pass — reset the cursor to 0 and re-run — which the archive's skip guard makes idempotent and cheap (already-archived bodies are skipped, only missing ones download). Reset it as the `bichon` user: writing it as root changes the owner of a file the service rewrites each run.
_Avoid_: checkpoint, offset, watermark (unqualified — always say message-date watermark), timestamp file.

**Tag Snapshot**:
A per-account `tags.json` beside the **Email Archive**, mapping RFC 5322 `Message-ID` → tags, rewritten in full on every archive run. The mutable-metadata counterpart to the Archive: tags are operator annotations applied _after_ ingestion and revised indefinitely, which an append-only store cannot represent. Keyed on `Message-ID` because Bichon regenerates its own envelope identifier on re-import. Costs one API call per run while no tags exist. See ADR-0012.
_Avoid_: tag index, label export, metadata sidecar (collides with the Archive's per-message `.meta.json`).

**Rebuild Latch**:
A file on the bichon Host (`/var/lib/bichon-uidvalidity-watch/rebuilds.log`) holding one line per detected **Internal Store** purge, written by an hourly timer that reads `bichon.service`'s journal from a saved cursor for Bichon's `detected with changed uid_validity` line. **Latched, not edge-triggered**: the unit exits non-zero while the file is non-empty, so the finding is republished every tick and clears only when an operator deletes the file — systemd clears a unit's failed state on the next successful start, so an exit code alone would erase the alert within the hour. The failed unit is the alert surface (`systemctl --failed`, Cockpit → Services); deleting the file is both the acknowledgement and the record that someone saw it. Exit codes match the **Backup Verdict**'s (0 clean, 1 recorded, 2 could not read the journal), so a watch that cannot read the journal is distinguishable from one that found nothing. Carries no delivery channel: it is a condition to be found, not a message that is sent. See ADR-0014.
_Avoid_: Alert file, flag, sentinel, marker (understates that the unit fails on it), monitor.

**Upstream Mailbox**:
The third-party IMAP (or Gmail-API) server that Bichon syncs _from_ — e.g. the operator's Gmail, Fastmail, ProtonMail Bridge endpoint. Distinct from the **Email Archive** (Bichon-side, append-only) and from any **Backup Recipe** target. Operations on the Upstream Mailbox (e.g. expunging old mail to reclaim quota) are out-of-scope for `auberge deploy` and `auberge backup`; any future tooling that touches it must treat it as authoritative-but-untrusted.
_Avoid_: IMAP server, mail provider, source mailbox.

**Synced Folder**:
A folder on an **Upstream Mailbox** that Bichon ingests into the **Email Archive**. The set is computed at **Account Reconcile** time as `(remote folder list) − (exclusion set)`, where the default exclusion set is `{Spam, Trash}` — folders whose user-meaning is "not real mail" or "I'm done with this", which an Archive must not invert. The result is written into Bichon's per-account `sync_folders` field. **Only a Synced Folder is an eligible expunge target**: the Archive can vouch only for folders it continuously ingests, so `examples/bichon-expunge.sh` neither offers nor accepts anything else — a folder outside the set (e.g. Trash, whose pre-exclusion sidecars linger in the append-only Archive) could pass a count-based coverage check on stale evidence.
_Avoid_: All folders, every folder, full mailbox, watched folder.

**Account Reconcile**:
The auberge-driven step that reads each Bichon account's live folder list (`GET /api/v1/list-mailboxes/<id>?remote=true`), computes the **Synced Folder** set, and PATCHes Bichon's per-account `sync_folders`. Account _creation_ (IMAP host, credentials, OAuth2 consent) remains UI-driven; reconcile is the only state auberge writes to Bichon. Folder identity is matched primarily by RFC 6154 SPECIAL-USE attributes (language-portable, hierarchy-portable), with case-insensitive name matching as fallback for legacy IMAP servers that don't advertise SPECIAL-USE.
_Avoid_: Account sync, account update, account migration.

**Expunge Sweep**:
One invocation of `examples/bichon-expunge.sh` that walks every eligible (account, **Synced Folder**) pair on a Bichon Host with a single expunge window, instead of one operator-chosen pair. The account set is (himalaya accounts ∩ **Email Archive** directories); the folder set per account is (Synced Folders ∩ **Upstream Mailbox** folders). Anything outside those intersections — an archive directory with no himalaya account, a himalaya account with no archive, a folder in Account Reconcile drift — is warned about, skipped, and named in the final report; only an empty target set aborts. Host-scoped gates (off-host backup, archive freshness) run once per sweep; coverage is proven per pair. Every gate runs before any deletion; the sweep then takes **two typed checkpoints** — the Bichon Host name (scope: which machine's accounts) and the grand message total (magnitude: proof the summary was read) — moving ADR-0007's human pause from per-folder to per-sweep while keeping its hard rules (TTY required, no `--yes`, no unattended path). A bare y/N is never a checkpoint: reflex-typable acknowledgements are exactly what the typed checksum exists to prevent. See ADR-0007 (amended 2026-08-03).
_Avoid_: All accounts / all folders (eligibility is the Synced Folder set, not the mailbox), batch mode, bulk expunge.

**Busy Feed**:
A privacy-sanitized iCalendar feed of the operator's busy intervals, derived from the operator's Host-side calendar sources — **Baikal**'s calendar data plus, optionally, a read-only external CalDAV calendar (e.g. iCloud) fetched on the Host — by a host-side script on a systemd timer and served publicly behind a secret token. Contains only opaque `Busy` `VEVENT`s (UTC start/end + a hashed per-instance UID); never event titles, locations, guests, descriptions, or the source UID. Sanitization happens on the Host, so no personal event content (and no external CalDAV credential) ever leaves the VPS — the feed is the privacy boundary. A **Busy Feed** is a tool-agnostic artifact like the **Email Archive**: auberge produces and serves it but does not ship its consumers (see ADR-0010).
_Avoid_: Free/busy feed (deliberately not a `VFREEBUSY` component — discrete `VEVENT`s carry per-instance UIDs for diffing), calendar sync, availability export.

**Actual**:
A Tailnet-only App — the Actual Budget **Sync Server**, deployed bare-metal from npm (`@actual-app/sync-server`, exact version pinned in role defaults, Node 22 from NodeSource; ADR-0016). Not the budget's store of record: every Actual client keeps a full local copy and replays change messages through the server. Its on-disk state (`/var/lib/actual`: `server-files/account.sqlite` for server accounts, password, and bank-sync credentials; `user-files/` for budget blobs) is what the Backup Recipe rsyncs — losing it costs the server password and **Enable Banking** credentials, never the budgets themselves.
_Avoid_: Actual Budget server (say Sync Server), budget database, budget backend.

**Sync Server**:
The relay-and-blob-store role Actual's server component plays: clients push and pull ordered change messages (optionally end-to-end encrypted, in which case the server cannot read budget contents) and upload whole-budget blobs for bootstrap of new devices. Syncing is convergence, not authority — any client can rebuild the server's copy by re-uploading.
_Avoid_: API server, backend, master copy.

**Enable Banking**:
The PSD2 open-banking aggregator Actual's bank sync uses on this deployment, chosen because GoCardless closed Bank Account Data to new signups in July 2025 (ADR-0016). Covers the operator's bank, C24 (AIS + PIS since October 2024). Configured once in Actual's UI — application ID plus credential file from Enable Banking's control panel; the credentials persist in **Actual**'s `account.sqlite`, inside the Backup Recipe and outside the Key Registry. Deliberately outside Actual's end-to-end encryption too: the server performs the bank pulls, so the credentials and in-flight transactions are server-readable by design — E2E protects the budget ledger at rest, not the bank-sync plumbing.
_Avoid_: bank API (unqualified), Nordigen/GoCardless successor (a distinct product, not a rebrand).

**Progress**:
The trait that runners (`AnsibleRunner`, `Recipe Executor`, `Backup Session`) emit events through. `TerminalProgress` is the production impl; tests use a `MockProgress`. Keeps runners free of terminal-output coupling.
_Avoid_: Logger, reporter

## Relationships

- A **Playbook** has exactly one **Playbook Meta** sibling.
- A **Playbook Meta** declares zero or more keys from the **Key Registry**.
- A **Playbook Meta** declares zero or one **Backup Recipe**.
- A **Preflight** binds one **Playbook Meta** to a validated **Config**.
- The **Recipe Executor** consumes one **Backup Recipe**; the **Backup Session** consumes many.
- A **Backup Verdict** reads only what a **Backup Session** already pushed, attributing a snapshot to a **Host** by the restic tag push writes (the same tag prune groups retention by).
- Bichon syncs from an **Upstream Mailbox** into its **Internal Store**; the **Email Archive** and **Tag Snapshot** are derived from the Internal Store; the **Backup Recipe** rsyncs those two and never the Internal Store.
- The **Email Archive** advances one **Archive Cursor** per account; mail that enters a **Synced Folder** below the cursor is archived only by an explicit full pass, never by the hourly increment.
- Restoring a purged **Internal Store** means replaying the **Email Archive** (bodies, foldered from the sidecars) and then the **Tag Snapshot** (tags), both via `examples/bichon-restore.sh`. Neither the Archive nor the Snapshot is searchable on its own.
- A purge of the **Internal Store** is announced by the **Rebuild Latch**, which detects it and nothing more: it neither prevents the purge nor triggers the replay.
- All runners report through **Progress**; none touch terminal output directly.
- An **App** is either a **Public App** or a **Tailnet-only App**, determined by the `tailnet_only` flag in its **Playbook Meta**. **DNS Publication** is dispatched accordingly.
- The **Busy Feed** is derived from **Baikal**'s calendar data — plus, optionally, a read-only external CalDAV calendar fetched Host-side — and served on Baikal's **Public App** site (Google's servers must reach it); auberge produces and serves it but ships no consumer.
- **Actual** relays sync between clients that each hold the full budget; its Backup Recipe path (`/var/lib/actual`) also carries the **Enable Banking** credentials, so restoring it restores bank sync.

## Example dialogue

> **Maintainer:** "Paperless needs a new env var. Where do I add it?"
> **Reviewer:** "Add it to the **Key Registry** with `secret: true` if it's sensitive, then list its name in `paperless.meta.yml` under `required_keys`. The next `auberge ansible run paperless` will fail-fast if the user hasn't set it."

> **Maintainer:** "Why doesn't the **Recipe Executor** know about restic?"
> **Reviewer:** "Restic push and prune are cross-recipe — they happen once per **Backup Session**, not once per **Backup Recipe**. The split is the whole reason those two modules exist."

## Flagged ambiguities

- "Backup runner" was used loosely for both per-recipe and multi-recipe execution. Resolved: use **Recipe Executor** (one recipe) and **Backup Session** (many recipes) — never "runner" without qualification.
- "Spec" was used early in the design conversation for what became **Playbook Meta**. Resolved: avoid "spec" — it conflicts with Rust's `cargo spec` and reads ambiguous next to "schema."
- "Append-only" was used of the **Email Archive** to mean "nothing is ever deleted," and was silently read as "everything is faithfully preserved." Resolved: it also means _nothing is ever updated_, which is correct for immutable bodies and wrong for mutable metadata. Say **append-only** of bodies and folder; say **snapshot** of tags (see **Tag Snapshot**, ADR-0012).
- "Write-once" was then read as a rule about the sidecar _file_, which would have forbidden repairing a field the file had never carried. Resolved: it is a rule about **observations** — a fact the archive learned from Bichon at first sight, `folder` being the only one. A **derived index key** recomputed from the immutable `.eml` is not an observation and may be written in place (ADR-0013), and neither is a payload that was never a message — replacing it completes a write that did not happen (ADR-0015).
- "Message id" was used for both the `<envelope-id>` in an `.eml` filename and the RFC 5322 `Message-ID`. Resolved: only the latter is an identity. Call the former the **envelope id** and treat it as a filename.

## Stdout discipline

**stdout is data; chrome goes to stderr.**

This rule follows [clig.dev](https://clig.dev/#output): programs should print only their primary data output to stdout so that output can be piped, redirected, or parsed (including as JSON) without noise. Status messages, hints, confirmations, and any other "chrome" must go to stderr.

In practice:

- `println!` and `print!` are allowed **only** in modules that emit the command's primary data output (e.g. `config_cmd`, `dns`, `headscale`, `host`, `select`, `backup`, and `output::print_table`).
- All other informational messages — "Cancelled.", spinner updates, success banners, hints, interactive prompts — must use `eprintln!`/`eprint!`, `output::info`, `output::success`, or `output::warn`, all of which write to stderr.
- Interactive prompts that read from stdin should `eprint!` the prompt and `io::stderr().flush()`, so the prompt is visible on the TTY even when the caller pipes stdout.

A CI step in `.github/workflows/master.yml` enforces this by failing if `println!`/`print!` appears in any source file outside the approved allowlist. The check is per-file, so modules on the allowlist are on the honor system: chrome that lives in an allowlisted file is not caught.
