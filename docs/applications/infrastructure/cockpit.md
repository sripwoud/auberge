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

Cockpit is deployed with the following defaults (override in `config.toml` or group vars):

| Variable | Default | Description |
|---|---|---|
| `cockpit_port` | `9090` | Port Cockpit listens on |
| `cockpit_tailscale_interface` | `tailscale0` | Network interface restricted to |

## Access

Cockpit is restricted to the Tailscale interface only via a UFW rule. It is **not** exposed to the public internet.

Access it at `https://<tailscale-ip>:9090` from any device on your tailnet.

> [!NOTE]
> Cockpit serves HTTPS with a self-signed TLS certificate by default. Your browser will show a certificate warning on first visit — this is expected and safe to accept. Since access is restricted to the Tailscale encrypted tunnel, the connection is already secured end-to-end.

Log in with your system user credentials (the admin user created during bootstrap).

## Features

- **Service management**: start/stop/restart any systemd unit from the browser
- **Log viewer**: journalctl with filtering
- **Resource monitoring**: real-time CPU, RAM, disk, and network graphs
- **Web terminal**: browser-based terminal when SSH is unavailable
- **Socket-activated**: consumes zero resources when not in use

## Security

- Access is restricted to the Tailscale interface (`tailscale0`) via UFW
- Port 9090 is blocked on all other interfaces
- No public internet exposure
- Authentication uses existing system user accounts

## Related

- [Tailscale](../networking/tailscale.md)
- [UFW](ufw.md)
- [Applications Overview](../overview.md)
