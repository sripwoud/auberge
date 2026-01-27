# Tailscale

Mesh VPN for secure remote access built on WireGuard.

Official Documentation: [https://tailscale.com](https://tailscale.com)

## Deployment

```bash
auberge ansible run --tags tailscale
```

## Configuration

Requires `TAILSCALE_AUTHKEY` environment variable. See [Environment Variables](../../configuration/environment-variables.md).

## Related

- [Environment Variables](../../configuration/environment-variables.md)
- [Applications Overview](../overview.md)
