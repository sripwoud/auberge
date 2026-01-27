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
- Use upstream DNS resolvers (Cloudflare, Google)

## Related

- [DNS Management](../../dns/managing-records.md)
- [Applications Overview](../overview.md)
