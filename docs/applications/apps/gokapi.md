# Gokapi

Expiring-link file sharing. Docs: [gokapi.readthedocs.io](https://gokapi.readthedocs.io), source: [github.com/Forceu/Gokapi](https://github.com/Forceu/Gokapi)

- **URL**: `https://{gokapi_subdomain}.{domain}` (default subdomain: `share`)
- **Data**: SQLite database + uploaded files under `/var/lib/gokapi/`
- **Pinned version**: 2.2.4

## Deploy

```bash
auberge deploy gokapi
```

## Required config

| Key                     | Purpose                         |
| ----------------------- | ------------------------------- |
| `gokapi_subdomain`      | Subdomain for HTTPS access      |
| `gokapi_admin_user`     | Superadmin username             |
| `gokapi_admin_password` | Superadmin password (bootstrap) |

## Notes

?> First deploy is single-pass and headless — `config.json` is templated and the superadmin is created via `gokapi --deployment-password` before the service starts. Caddy and DNS land in the same run. See [ADR-0009](https://github.com/sripwoud/auberge/blob/master/meta/adr/0009-gokapi-headless-bootstrap.md).

?> Rotating `gokapi_admin_password` is not automatic. Delete the bootstrap marker on the host and redeploy — see [role README](https://github.com/sripwoud/auberge/blob/master/ansible/roles/gokapi/README.md#rotating-the-admin-password).

Replaces the removed `webdav` role for the link-share use case. Gokapi is **not** a WebDAV server — it does not expose its storage over the WebDAV protocol. If you need mountable network storage, use [Syncthing](syncthing.md) instead.
