# auberge ansible bootstrap

Bootstrap a fresh VPS: create the `ansible` user, harden SSH, change the SSH port, configure UFW. Run **once per VPS**. Alias: `auberge a b`.

```bash
auberge ansible bootstrap [HOST] [OPTIONS]
```

!> Configure your VPS provider's firewall to allow your custom `ssh_port` **before** running. Bootstrap changes the SSH port — without the firewall rule, you're locked out.

## Options

| Option        | Description                         | Default |
| ------------- | ----------------------------------- | ------- |
| `--port PORT` | SSH port for initial connection     | `22`    |
| `--ip IP`     | Target IP (required with `--force`) | Prompt  |
| `-f, --force` | Skip confirmation prompts (CI/CD)   | `false` |

## Prerequisites

```bash
auberge host add my-vps 203.0.113.10 --user ansible
auberge config set hostname my-vps
auberge config set admin_user_name yourname
auberge config set ssh_port 22022
auberge config set admin_user_password your-password    # optional, for Cockpit web login
```

You also need either password access or a pre-authorized SSH key for the bootstrap user (typically `root`).

## Examples

```bash
auberge ansible bootstrap my-vps --ip 203.0.113.10
auberge ansible bootstrap my-vps --ip 203.0.113.10 --port 22222
auberge ansible bootstrap my-vps --ip 203.0.113.10 --force      # non-interactive
```

## What it does

1. Connect as bootstrap user (root) on port 22.
2. Create `ansible` user (passwordless sudo) and `{admin_user_name}` (full sudo).
3. Apply SSH hardening: custom port, disable root login, disable password auth.
4. Validate SSH on the new port — only then enable UFW.

## Troubleshooting

- **Locked out**: provider firewall blocks the new port. Use the provider's web/VNC console: `sudo ufw disable`, fix SSH, re-enable.
- **`User already exists`**: bootstrap already ran. Use `auberge ansible run` for subsequent changes.
