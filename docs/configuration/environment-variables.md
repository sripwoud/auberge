# Environment Variables

All configuration is managed via `mise.toml` environment variables.

## Secrets (encrypted with age)

Set encrypted secrets using mise:

```bash
mise set --age-encrypt --prompt ADMIN_USER_NAME
mise set --age-encrypt --prompt ADMIN_USER_EMAIL
mise set --age-encrypt --prompt PRIMARY_DOMAIN
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
mise set --age-encrypt --prompt BAIKAL_ADMIN_PASSWORD
mise set --age-encrypt --prompt WEBDAV_PASSWORD
mise set --age-encrypt --prompt TAILSCALE_AUTHKEY
mise set --age-encrypt --prompt SSH_PORT
mise set --age-encrypt --prompt AUBERGE_HOST
mise set --age-encrypt --prompt VIBECODER_HOST
mise set --age-encrypt --prompt OPENCLAW_GATEWAY_TOKEN
mise set --age-encrypt --prompt CLAUDE_AI_SESSION_KEY
mise set --age-encrypt --prompt CLAUDE_WEB_SESSION_KEY
mise set --age-encrypt --prompt CLAUDE_WEB_COOKIE
```

## Public Configuration

Already defined in `mise.toml` [env] section:

- `DNS_DEFAULT_TTL` - DNS record TTL in seconds (default: 300)
- `BAIKAL_SUBDOMAIN` - Subdomain for Baikal (default: calendrier)
- `BLOCKY_SUBDOMAIN` - Subdomain for Blocky DNS (default: dns)
- `CALIBRE_SUBDOMAIN` - Subdomain for Calibre (default: lire)
- `COLPORTEUR_SUBDOMAIN` - Subdomain for Colporteur (default: feeds)
- `FRESHRSS_SUBDOMAIN` - Subdomain for FreshRSS (default: rss)
- `NAVIDROME_SUBDOMAIN` - Subdomain for Navidrome (default: musique)
- `WEBDAV_SUBDOMAIN` - Subdomain for WebDAV (default: webdav)
- `YOURLS_SUBDOMAIN` - Subdomain for YOURLS (default: url)

## Viewing Configuration

```bash
# View all environment variables
mise env

# View specific variable
mise env | grep SSH_PORT
```

## API Keys and Tokens

### Cloudflare API Token

Required for DNS-01 ACME challenges via Lego certificate automation:

1. Log into [Cloudflare Dashboard](https://dash.cloudflare.com)
2. Navigate to: My Profile → API Tokens → Create Token
3. Use the "Edit zone DNS" template
4. Configure permissions:
   - Zone → DNS → Edit
   - Zone → Zone → Read
5. Set zone resources to your domain
6. Copy the token and add to mise:
   ```bash
   mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
   ```

IP whitelisting is optional (all IPs are allowed by default).

### Tailscale

Required for VPN mesh networking:

1. Generate auth key at [Tailscale Admin Console](https://login.tailscale.com/admin/settings/keys)
2. Set reusable and ephemeral flags as needed
3. Store the key:
   ```bash
   mise set --age-encrypt --prompt TAILSCALE_AUTHKEY
   ```

## Related Documentation

- [Secrets Management](secrets.md) - age encryption
- [Development Setup](../development/setup.md) - Local development
