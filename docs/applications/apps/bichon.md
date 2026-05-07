# Bichon

Email archiving service with continuous IMAP sync and full-text search.

Official Documentation: [https://github.com/rustmailer/bichon](https://github.com/rustmailer/bichon)

## Deployment

```bash
auberge ansible run --tags bichon
```

Bare-metal deployment (no Docker). Requires the [Tailscale](../networking/tailscale.md) role to be deployed first.

## Configuration

Required variables in `config.toml`:

- `bichon_encryption_password` - Encryption password for stored credentials (IMAP passwords, OAuth tokens, login passwords) and metadata database. Email content is stored as zstd-compressed blocks and is **not** encrypted by this password.
- `bichon_subdomain` - Subdomain for HTTPS access (e.g. `bichon`)
- `bichon_api_token` - Bearer token used by the hourly archive timer to call Bichon's REST API. Mint one in Bichon's UI after the first deploy, paste it into `config.toml`, then re-run the role.

Optional:

- `bichon_tailscale_ip` - Override the host's cached Tailscale IP for this app specifically (e.g. `100.x.y.z`). Usually unnecessary: bichon's playbook meta declares `tailnet_only: true`, so `dns set-all --host <name>` already auto-fills the IP from `host.tailscale_ip` (set once via [`auberge host detect-tailscale-ip`](../../cli-reference/host/detect-tailscale-ip.md)). Use this key only when bichon needs a _different_ Tailscale IP than the rest of the host's tailnet-only apps.

> **Warning**: Once the encryption password is set, it **cannot be changed**. Changing it later will make all encrypted data unreadable. To start over, you must reinitialize Bichon and delete all emails and metadata. See [upstream docs](https://github.com/rustmailer/bichon/wiki/Setting-the-Bichon-Encryption-Password). The Ansible role enforces this: the password file is written once on first deploy, and subsequent runs will fail if the configured password differs from the deployed one.

## Architecture

- **Runtime**: Rust single binary
- **Search**: Tantivy (embedded full-text search, no external DB)
- **Encryption**: Credential and metadata encryption (email content is not encrypted)
- **Config**: `/opt/bichon/bichon.env`
- **Data**: `/opt/bichon/data` (Bichon's encrypted internal store — not backed up)
- **Archive**: `/var/lib/bichon-archive` (per-message `.eml` mirror, hourly; backed up)

### Systemd Services

| Service                  | Description                                           |
| ------------------------ | ----------------------------------------------------- |
| `bichon`                 | Web server and IMAP sync daemon                       |
| `bichon-archive.service` | Walks Bichon's REST API, writes EML files (`oneshot`) |
| `bichon-archive.timer`   | Triggers the archive hourly with 10min jitter         |

## Access

Accessible at `https://bichon.<domain>` (or whichever subdomain you configured) — but only from Tailscale network members. The DNS A record points to the server's Tailscale IP (a CGNAT address in `100.64.0.0/10`), which is not routable from the public internet. Caddy binds this vhost exclusively to the Tailscale interface, so it cannot be reached via the server's public IP.

Caddy obtains a valid Let's Encrypt certificate via DNS-01 challenge (Cloudflare API), so the connection uses real HTTPS even though the subdomain resolves to a private Tailscale IP.

Default credentials: `admin` / `admin@bichon`. Change these after first login.

Requires Tailscale — the service will not start without `tailscaled.service`.

See [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains) for the generic pattern.

## Email Archive

A `bichon-archive.timer` on the host runs hourly and walks Bichon's REST API to mirror each message as a plaintext `.eml` file under `/var/lib/bichon-archive/<account-email>/<YYYY>/<MM>/<message-id>.eml`, with a `<message-id>.meta.json` sidecar capturing folder name and tags. Per-account cursors under `.state/` plus a 24-hour overlap window keep incremental runs cheap. Atomic per-message writes; failures advance no cursor.

Verify after the first deploy:

```bash
sudo systemctl start bichon-archive.service   # seed the archive immediately
sudo systemctl list-timers bichon-archive.timer
sudo find /var/lib/bichon-archive -name '*.eml' | wc -l
```

The archive is consumable without Bichon — any IMAP/MBOX-aware client (Thunderbird, mutt) can ingest the `.eml` tree directly. The non-rotatable `bichon_encryption_password` is **not** required to read it.

## Backup

Supported via `auberge backup create --apps bichon`. The Backup Recipe rsyncs `/var/lib/bichon-archive` (the EML archive above), **not** Bichon's internal `/opt/bichon/data` store. This makes the backup tool-agnostic, restic-friendly (Tantivy's segment-rewrite churn no longer dominates dedup), and survives any future where Bichon stops being maintainable.

The timer must have run at least once before the first backup, otherwise the bichon backup will be empty. Rationale and considered alternatives in [ADR-0006](https://github.com/sripwoud/auberge/blob/master/meta/adr/0006-bichon-archive-feeds-backup-recipe.md).

See [Backup & Restore](../../backup-restore/overview.md).

## MBOX Import

Bichon supports importing EML, MBOX, and PST archives. Refer to the [upstream documentation](https://github.com/rustmailer/bichon) for current import commands and usage, as the tooling may change between versions.

## First-time account setup

1. Open the Bichon UI at `https://bichon.<domain>` and navigate to **Accounts → Add account**.
2. Enter your IMAP credentials (username, password, host, port) or complete the OAuth2 flow.
3. After saving, synchronise folders:

   **Preferred (once `reconcile-folders` lands — issue #329):**

   ```bash
   auberge bichon reconcile-folders --host <hostname> --apply
   ```

   This command ticks every folder except those bearing RFC 6154 `SPECIAL-USE`
   attributes `\Junk` and `\Trash` (and their provider-specific equivalents:
   `Spam`, `Junk Mail`, `Deleted Items`, `Bin`, `Papierkorb`, `Pourriels`, …).
   See [ADR-0007](https://github.com/sripwoud/auberge/blob/master/meta/adr/0007-bichon-folder-reconcile-scope-and-silent-vs-loud.md)
   for the full exclusion rationale.

   **Until `reconcile-folders` ships:**

   In the Bichon UI go to **Accounts → \<account\> → Folders** and tick every
   folder **except** `Spam`/`Junk`/`Junk Mail` and `Trash`/`Deleted Items`
   (and any local-language equivalents your provider uses).  The rule is:
   exclude RFC 6154 `\Junk` and `\Trash`; include everything else.

4. Seed the archive immediately so the first backup is not empty:

   ```bash
   sudo systemctl start bichon-archive.service
   sudo journalctl -u bichon-archive.service -f
   ```

## Archived-then-expunge workflow

### Why this exists

The **Email Archive** is append-only (ADR-0006).  It is the enabling primitive
for safely deleting mail from the **Upstream Mailbox**: once a message is in
the archive _and_ the archive is in an off-host backup, expunging it from the
IMAP server does not lose it.

Auberge does **not** ship an expunge tool (ADR-0007 silent-vs-loud principle).
The destructive step is a deliberate, operator-owned action that must remain
explicit.

### Ordering invariant

Execute in this order and check each precondition before proceeding:

1. **Bichon ingests** — the account's folders are ticked in the Bichon UI and
   `bichon.service` is syncing normally.
2. **`bichon-archive.timer` runs successfully** — verify with
   `journalctl -u bichon-archive.service`.  The journal is the source of truth;
   archive recency alone is not sufficient (see foot-gun warning below).
3. **`auberge backup sync` (or equivalent) runs** — the archive must be in an
   off-host backup before expunge.
4. **Operator expunges** — using `himalaya` or any other IMAP client.

### Foot-gun warnings

> ⚠️ **Cursor-failure masking.** `bichon-archive.sh` advances the cursor only
> on a fully successful run.  A non-failing run immediately after a failing one
> can appear to have covered the gap, but the failing run's messages may be
> absent.  Always check `journalctl -u bichon-archive.service` for errors
> before expunging — do not rely on the archive's modification time or message
> count alone.

> ⚠️ **Unticked folders are not archived.** The archive captures only what
> Bichon ingested.  If a folder was unticked in Bichon's UI, messages in that
> folder are **not** in the archive.  Verify `sync_folders` before expunging
> any folder.

> ⚠️ **Do not automate expunge on a cron.**  The human pause between
> "archive confirmed" and "expunge" is the safety net.  Automating it removes
> the only recovery window for a misconfiguration or a failed archive run.

### Recommended tooling

| Task | Tool |
| ---- | ---- |
| IMAP-side ops (list, search, expunge) | [`himalaya`](https://github.com/pimalaya/himalaya) — Rust, matches project ethos |
| Archive checks | `ssh <host> find /var/lib/bichon-archive -name '*.eml' \| wc -l` |
| Archive freshness | `journalctl -u bichon-archive.service` |
| Off-host backup | `auberge backup create --apps bichon` |

### Reference shell script

`examples/bichon-expunge.sh` is a worked example for MXroute that:

1. Checks `bichon-archive.service` journal freshness.
2. Counts IMAP messages in a configurable time window.
3. Counts `.eml` files in the archive for the same window.
4. Stops if there is a coverage gap.
5. **Prints** the expunge command without running it.

The script is version-controlled as a reference but is **not** shipped in the
`auberge` binary.

```bash
BICHON_HOST=yourserver \
BICHON_ARCHIVE_PATH=/var/lib/bichon-archive \
IMAP_ACCOUNT=you@example.com \
IMAP_HOST=mail.mxrouting.net \
FOLDER=INBOX \
WINDOW_DAYS=90 \
bash examples/bichon-expunge.sh
```

## Related

- [Tailscale](../networking/tailscale.md)
- [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
- [ADR-0006 — Bichon archive feeds Backup Recipe](https://github.com/sripwoud/auberge/blob/master/meta/adr/0006-bichon-archive-feeds-backup-recipe.md)
- [ADR-0007 — Folder reconcile scope and silent-vs-loud principle](https://github.com/sripwoud/auberge/blob/master/meta/adr/0007-bichon-folder-reconcile-scope-and-silent-vs-loud.md)
