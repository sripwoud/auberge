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

Gokapi's `/setup` endpoint is unauthenticated until completion. To prevent a takeover race, the role gates public exposure on `config.json` existing:

- **First deploy**: installs the binary + systemd unit only. No Caddy site, no DNS A record. Gokapi listens on `:53842` but UFW blocks it from the public internet.
- **Complete setup via SSH tunnel**:
  ```bash
  ssh -L 53842:127.0.0.1:53842 <ansible-user>@<host>
  # open http://localhost:53842/setup in your browser
  ```
  Enter the admin credentials matching `gokapi_admin_user` / `gokapi_admin_password` from `config.toml`. Pick SQLite, save.
- **Second deploy** (`auberge deploy --tags gokapi`): role detects `config.json`, deploys the Caddy site and creates the Cloudflare A record. `https://{gokapi_subdomain}.{domain}` is now live.

To rotate the admin password later, run on the host: `gokapi --deployment-password <new-password>`.

## Notes

- Sits behind Caddy with `GOKAPI_USE_CLOUDFLARE=true` and `GOKAPI_TRUSTED_PROXIES=127.0.0.1`.
- Replaces the removed `webdav` role for the link-share use case. Gokapi is **not** a WebDAV server — it does not expose its storage over the WebDAV protocol. If you need mountable network storage, use [Syncthing](syncthing.md) instead.
