# Headscale

Self-hosted Tailscale control server. Replaces the Tailscale SaaS coordination plane so all device metadata stays on your infrastructure.

Official Documentation: [https://headscale.net](https://headscale.net)

## Deployment

```bash
auberge ansible run --tags headscale
```

Headscale deploys to the infrastructure layer (before Tailscale). After initial deployment, generate a pre-auth key and configure the Tailscale role to use it:

```bash
# Create a user and pre-auth key locally:
auberge headscale add-user --host myserver

# Configure Tailscale to use Headscale:
auberge config set tailscale_authkey <HEADSCALE_PREAUTH_KEY>
auberge config set tailscale_login_server https://hs.<your-domain>
auberge ansible run --tags tailscale
```

## Configuration

Required variables in `config.toml`:

- `headscale_subdomain` - Subdomain for the control plane endpoint (e.g. `hs`)

Optional:

- `tailscale_login_server` - Headscale URL passed to `tailscale up --login-server`. When set, nodes connect to Headscale instead of Tailscale SaaS.

See [Environment Variables](../../configuration/environment-variables.md).

## Architecture

- **Runtime**: Single Go binary with embedded SQLite
- **Config**: `/etc/headscale/config.yaml`
- **Data**: `/var/lib/headscale` (SQLite DB, noise keys)
- **DERP**: Embedded relay (region 999, STUN on port 3478/UDP)

### Systemd Services

| Service     | Description                            |
| ----------- | -------------------------------------- |
| `headscale` | Control server, API, and embedded DERP |

## Access

Accessible at `https://hs.<domain>` (or whichever subdomain you configured) from the public internet. Unlike other services that bind to Tailscale IPs, Headscale must be publicly reachable — Tailscale clients need to contact it before they can join the tailnet.

Caddy reverse-proxies to `127.0.0.1:8080` with automatic HTTPS. UFW allows port 3478/UDP for STUN (NAT traversal for the embedded DERP relay).

## Migration from Tailscale SaaS

1. Deploy Headscale: `auberge ansible run --tags headscale`
2. Create a user and pre-auth key: `auberge headscale add-user --host myserver`
3. Update config:
   ```bash
   auberge config set tailscale_authkey <NEW_KEY>
   auberge config set tailscale_login_server https://hs.<domain>
   ```
4. On each node: `tailscale logout`, then re-run `auberge ansible run --tags tailscale`
5. Verify: `tailscale status` shows all nodes connected via Headscale

Existing services (Paperless, Bichon, OpenClaw) continue working unchanged — same Tailscale client, same WireGuard data plane.

## Backup

Supported via `auberge backup create --apps headscale`. Backs up `/var/lib/headscale` including the SQLite database and cryptographic keys. No external database.

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Tailscale](./tailscale.md)
- [Environment Variables](../../configuration/environment-variables.md)
- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
- [CLI Reference: headscale](../../cli-reference/headscale/add-user.md)
