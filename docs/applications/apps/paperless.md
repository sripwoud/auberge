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

Available only via the Tailscale network at `http://<tailscale-ip>:8000`. Not publicly accessible (no Caddy reverse proxy). Requires Tailscale — services will not start without `tailscaled.service`.

## Backup

Supported via `auberge backup create --apps paperless`. Backs up data and media directories only. The PostgreSQL database is **not** included and must be backed up separately (see [#105](https://github.com/sripwoud/auberge/issues/105)).

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Tailscale](../networking/tailscale.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
