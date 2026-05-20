# Gokapi

Expiring-link file sharing. Docs: [gokapi.readthedocs.io](https://gokapi.readthedocs.io), source: [github.com/Forceu/Gokapi](https://github.com/Forceu/Gokapi)

- **URL**: `https://{gokapi_subdomain}.{domain}` (default subdomain: `share`)
- **Data**: SQLite database + uploaded files under `/var/lib/gokapi/`
- **Pinned version**: 2.2.4

## Deploy

```bash
auberge deploy gokapi
```

## First-deploy setup

Gokapi has no environment-variable bootstrap for admin credentials. After the first deploy, visit `https://{gokapi_subdomain}.{domain}/setup` and enter the admin username and password from `gokapi_admin_user` / `gokapi_admin_password` in `config.toml`. The wizard disappears once `config.json` exists. Subsequent deploys are idempotent.

To rotate the password later, run on the host: `gokapi --deployment-password <new-password>`.

## Notes

- Sits behind Caddy with `GOKAPI_USE_CLOUDFLARE=true` and `GOKAPI_TRUSTED_PROXIES=127.0.0.1`.
- Replaces the removed `webdav` role for the link-share use case. Gokapi is **not** a WebDAV server — it does not expose its storage over the WebDAV protocol. If you need mountable network storage, use [Syncthing](syncthing.md) instead.
