# Configuration

All configuration is managed via `~/.config/auberge/config.toml` and `~/.config/auberge/hosts.toml`.

## Config Management

```bash
auberge config init
auberge config list
auberge config set <key> <value>
auberge config get <key>
```

## Host Management

```bash
auberge host add <name> <ip> --user <user> --port <port>
auberge host list
auberge host show <name>
```

## Key Configuration Values

### Identity (config.toml)

```bash
auberge config set admin_user_name <username>
auberge config set admin_user_email <email>
auberge config set primary_domain <domain>
auberge config set ssh_port <port>
```

### API Tokens (config.toml)

```bash
auberge config set cloudflare_dns_api_token <token>
auberge config set tailscale_authkey <key>
```

### Application Settings (config.toml)

Subdomain configuration for each service:

```bash
auberge config set baikal_subdomain calendrier
auberge config set blocky_subdomain dns
auberge config set freshrss_subdomain rss
auberge config set navidrome_subdomain musique
auberge config set webdav_subdomain webdav
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
6. Store the token:
   ```bash
   auberge config set cloudflare_dns_api_token <TOKEN>
   ```

### Tailscale

Required for VPN mesh networking:

1. Generate auth key at [Tailscale Admin Console](https://login.tailscale.com/admin/settings/keys)
2. Set reusable and ephemeral flags as needed
3. Store the key:
   ```bash
   auberge config set tailscale_authkey <KEY>
   ```

## Related Documentation

- [Secrets Management](./secrets.md) - config.toml security
- [Hosts Configuration](./hosts.md) - hosts.toml management
- [Development Setup](../development/setup.md) - Local development
