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

| Variable            | Default   | Description                              |
| ------------------- | --------- | ---------------------------------------- |
| `cockpit_port`      | `9090`    | Port Cockpit listens on (localhost only) |
| `cockpit_subdomain` | `cockpit` | Subdomain for Cockpit                    |

## Access

Cockpit is proxied through Caddy with a real TLS certificate, bound to the Tailscale interface only — not exposed to the public internet.

Access it at `https://cockpit.<your-domain>` from any device on your tailnet. To log in, you must set `admin_user_password` in `config.toml` and re-run bootstrap — Cockpit requires a Linux password for authentication.

Cockpit itself listens on `127.0.0.1:9090` (localhost only) via a systemd socket override. Caddy terminates TLS and reverse proxies to it.

## Related

- [Caddy](caddy.md)
- [Tailscale](../networking/tailscale.md)
- [Applications Overview](../overview.md)
