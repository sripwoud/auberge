# UFW

Uncomplicated Firewall - simple firewall configuration for Linux.

Official Documentation: [https://launchpad.net/ufw](https://launchpad.net/ufw)

## Deployment

```bash
auberge ansible run --tags ufw
```

## Configuration

Auberge configures UFW to allow:

- Custom SSH port
- HTTP (80)
- HTTPS (443)
- DNS over TLS (853)

All other ports are blocked by default.

## Related

- [Applications Overview](../overview.md)
