# Caddy

Reverse proxy with automatic HTTPS certificate management via Let's Encrypt.

Official Documentation: [https://caddyserver.com](https://caddyserver.com)

## Deployment

```bash
auberge ansible run --tags caddy
```

## Configuration

Auberge configures Caddy to:

- Automatically provision SSL certificates via DNS-01 challenge (Cloudflare)
- Proxy requests to backend services
- Handle HTTPS redirects

## Related

- [Cloudflare Setup](../../dns/cloudflare-setup.md)
- [Applications Overview](../overview.md)
