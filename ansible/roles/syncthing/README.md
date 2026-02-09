# Syncthing Role

Installs and configures [Syncthing](https://syncthing.net/) for file synchronization.

## Features

- Installs Syncthing from official APT repository
- Enables and starts Syncthing as a systemd service instance (running under the `syncthing_user` account)
- Optionally configures OpenClaw workspace folder for sync
- Supports remote web UI access (optional)

## Variables

| Variable                          | Default                            | Description                               |
| --------------------------------- | ---------------------------------- | ----------------------------------------- |
| `syncthing_user`                  | `admin_user_name` / `ansible_user` | User to run Syncthing as                  |
| `syncthing_config_path`           | `~/.local/state/syncthing`         | Syncthing config directory                |
| `syncthing_listen_all_interfaces` | `false`                            | Listen on 0.0.0.0 instead of 127.0.0.1    |
| `syncthing_configure_workspace`   | `false`                            | Auto-configure OpenClaw workspace folder  |
| `syncthing_workspace_id`          | `openclaw-workspace`               | Folder ID in Syncthing                    |
| `syncthing_workspace_label`       | `OpenClaw Workspace`               | Folder label                              |
| `syncthing_workspace_path`        | `~/.openclaw/workspace`            | Path to sync                              |
| `syncthing_device_id`             | `""`                               | Device ID to share folder with (optional) |

## Usage

Add to your playbook:

```yaml
- role: syncthing
  tags: [apps, sync, syncthing]
```

### Remote Web UI Access

The recommended way to access the Syncthing web UI is via SSH port forwarding:

```bash
ssh -L 8384:localhost:8384 user@vps
# Then access http://localhost:8384 locally
```

If SSH tunneling is not practical, you can expose the GUI on all interfaces:

```yaml
- role: syncthing
  vars:
    syncthing_listen_all_interfaces: true
```

**Security warning:** Exposing the Syncthing GUI on all interfaces without authentication allows anyone who can reach port 8384 to control your Syncthing instance. If you must use this option:

1. Configure GUI authentication (username/password) in the Syncthing web UI immediately
2. Restrict access with a firewall rule (e.g., `ufw allow from YOUR_IP to any port 8384`)

## Post-Install

After installation:

1. Access web UI (http://localhost:8384 or via SSH tunnel)
2. Note the device ID shown in web UI
3. Install Syncthing on desktop/mobile
4. Add VPS as remote device (use device ID)
5. Share `openclaw-workspace` folder between devices

## Tags

- `apps` - Application installation
- `sync` - File synchronization
- `syncthing` - Syncthing-specific tasks
