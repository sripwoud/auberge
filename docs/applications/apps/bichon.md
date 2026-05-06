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

## Related

- [Tailscale](../networking/tailscale.md)
- [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
