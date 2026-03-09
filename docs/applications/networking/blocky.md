# Blocky

DNS server with integrated ad and tracking blocking capabilities.

Official Documentation: [https://0xerr0r.github.io/blocky](https://0xerr0r.github.io/blocky)

## Deployment

```bash
auberge ansible run --tags blocky
```

## Configuration

Auberge configures Blocky to:

- Provide DNS resolution with ad-blocking
- Support DNS over TLS (DoT) on port 853
- Listen on port 53 on the Tailscale interface for tailnet DNS queries
- Listen on port 5353 for general DNS queries
- Use upstream DNS resolvers (Cloudflare, Google)

## Tailscale DNS Integration

When `tailscale_api_key` is set in `config.toml`, Blocky automatically registers itself as the tailnet DNS nameserver via the Tailscale API. This means all devices on the tailnet get ad-blocking DNS without manual configuration.

The deployment sequence:

1. Blocky binds to port 53 on the server's Tailscale IP
2. Blocky waits until the port is listening
3. Blocky calls the Tailscale API to set itself as the DNS nameserver
4. MagicDNS is enabled to activate DNS override for all tailnet clients

See [Environment Variables](../../configuration/environment-variables.md#tailscale-api-key) for API key setup.

## Related

- [Tailscale](./tailscale.md)
- [DNS Management](../../dns/managing-records.md)
- [Applications Overview](../overview.md)
