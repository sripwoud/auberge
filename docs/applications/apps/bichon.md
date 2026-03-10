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

- `bichon_encryption_password` - Encryption password for at-rest email encryption
- `bichon_subdomain` - Subdomain for HTTPS access (e.g. `bichon`)

Optional:

- `bichon_tailscale_ip` - Override auto-detected Tailscale IP (e.g. `100.x.y.z`); signals `dns set-all` to use this IP instead of the public IP

See [Environment Variables](../../configuration/environment-variables.md).

## Architecture

- **Runtime**: Rust single binary
- **Search**: Tantivy (embedded full-text search, no external DB)
- **Encryption**: Mandatory at-rest encryption
- **Config**: `/opt/bichon/bichon.env`
- **Data**: `/opt/bichon/data`

### Systemd Services

| Service  | Description                     |
| -------- | ------------------------------- |
| `bichon` | Web server and IMAP sync daemon |

## Access

Accessible at `https://bichon.<domain>` (or whichever subdomain you configured) — but only from Tailscale network members. The DNS A record points to the server's Tailscale IP (a CGNAT address in `100.64.0.0/10`), which is not routable from the public internet. Caddy binds this vhost exclusively to the Tailscale interface, so it cannot be reached via the server's public IP.

Caddy obtains a valid Let's Encrypt certificate via DNS-01 challenge (Cloudflare API), so the connection uses real HTTPS even though the subdomain resolves to a private Tailscale IP.

Default credentials: `admin` / `admin@bichon`. Change these after first login.

Requires Tailscale — the service will not start without `tailscaled.service`.

See [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains) for the generic pattern.

## Backup

Supported via `auberge backup create --apps bichon`. Backs up the entire data directory including search indices and archived emails. No external database — all data is self-contained on disk.

See [Backup & Restore](../../backup-restore/overview.md).

## MBOX Import

Existing Thunderbird archives can be imported using the `bichonctl` CLI tool on the server:

1. Copy `.mbox` files to the server: `scp archive.mbox user@host:/tmp/`
2. Run the import on the server (as the bichon user or via sudo)
3. Verify imported emails via the web UI search

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Tailscale](../networking/tailscale.md)
- [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
