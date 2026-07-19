# Paperless-ngx

Document management system for organizing and searching scanned documents. Docs: [docs.paperless-ngx.com](https://docs.paperless-ngx.com)

- **URL**: tailnet only — see [Tailnet-only apps](cli-reference/dns/set-all.md#tailnet-only-apps)
- **Port**: internal (Caddy proxy)
- **Data**: `/opt/paperless/data`, `/opt/paperless/media`

## Deploy

```bash
auberge deploy paperless
```

Bare-metal (no Docker). Requires Tailscale deployed first.

## Required config

| Key                        | Purpose                                                             |
| -------------------------- | ------------------------------------------------------------------- |
| `paperless_secret_key`     | Django secret key                                                   |
| `paperless_admin_user`     | Admin username                                                      |
| `paperless_admin_password` | Admin password                                                      |
| `paperless_db_password`    | PostgreSQL password                                                 |
| `paperless_subdomain`      | Subdomain for HTTPS access                                          |
| `paperless_tailscale_ip`   | Tailscale IP — signals `dns set-all` to use it instead of public IP |

## Notes

!> `auberge backup create --apps paperless` backs up data and media directories only. The PostgreSQL database is **not** included — see [#105](https://github.com/sripwoud/auberge/issues/105).
