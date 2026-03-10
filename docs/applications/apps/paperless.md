# Paperless-ngx

Document management system for organizing and searching scanned documents.

Official Documentation: [https://docs.paperless-ngx.com](https://docs.paperless-ngx.com)

## Deployment

```bash
auberge ansible run --tags paperless
```

Bare-metal deployment (no Docker). Requires the [Tailscale](../networking/tailscale.md) role to be deployed first.

## Configuration

Requires the following variables in `config.toml`:

- `paperless_secret_key` - Django secret key
- `paperless_admin_user` - Admin username
- `paperless_admin_password` - Admin password
- `paperless_db_password` - PostgreSQL password
- `paperless_subdomain` - Subdomain for HTTPS access (e.g. `paperless`)
- `paperless_tailscale_ip` - Tailscale IP of the server (e.g. `100.x.y.z`); signals `dns set-all` to use the Tailscale IP instead of the public IP

See [Environment Variables](../../configuration/environment-variables.md).

## Architecture

- **Database**: PostgreSQL
- **Cache**: Redis
- **OCR**: Tesseract
- **Config**: `/opt/paperless/paperless.conf`
- **Data**: `/opt/paperless/data`
- **Media**: `/opt/paperless/media`

### Systemd Services

| Service                | Description           |
| ---------------------- | --------------------- |
| `paperless-webserver`  | Granian web server    |
| `paperless-consumer`   | Document consumer     |
| `paperless-task-queue` | Celery task queue     |
| `paperless-scheduler`  | Celery Beat scheduler |

## Access

Accessible at `https://paperless.<domain>` (or whichever subdomain you configured) — but only from Tailscale network members. The DNS A record points to the server's Tailscale IP (a CGNAT address in `100.64.0.0/10`), which is not routable from the public internet. Caddy binds this vhost exclusively to the Tailscale interface, so it cannot be reached via the server's public IP.

Caddy obtains a valid Let's Encrypt certificate via DNS-01 challenge (Cloudflare API), so the connection uses real HTTPS even though the subdomain resolves to a private Tailscale IP.

Requires Tailscale — services will not start without `tailscaled.service`.

See [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains) for the generic pattern.

## Backup

Supported via `auberge backup create --apps paperless`. Backs up data and media directories only. The PostgreSQL database is **not** included and must be backed up separately (see [#105](https://github.com/sripwoud/auberge/issues/105)).

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Tailscale](../networking/tailscale.md)
- [Tailnet-only subdomains](../../dns/batch-operations.md#tailnet-only-subdomains)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
