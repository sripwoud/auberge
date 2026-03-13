# Cockpit

Web-based Linux server administration panel providing browser access to systemd services, logs, resource monitoring, and a terminal.

Official Documentation: [https://cockpit-project.org](https://cockpit-project.org)

## Deployment

```bash
auberge deploy cockpit
# or
auberge ansible run --tags cockpit
```

## Configuration

| Variable                      | Default      | Description                     |
| ----------------------------- | ------------ | ------------------------------- |
| `cockpit_port`                | `9090`       | Port Cockpit listens on         |
| `cockpit_tailscale_interface` | `tailscale0` | Network interface restricted to |

## Access

Cockpit is restricted to the Tailscale interface only via a UFW rule — not exposed to the public internet.

Access it at `https://<tailscale-ip>:9090` from any device on your tailnet. Log in with your system user credentials (the admin user created during bootstrap).

> [!NOTE]
> Cockpit serves HTTPS with a self-signed certificate. Your browser will show a warning on first visit — safe to accept since the Tailscale tunnel already provides end-to-end encryption.

## Related

- [Tailscale](../networking/tailscale.md)
- [UFW](ufw.md)
- [Applications Overview](../overview.md)
