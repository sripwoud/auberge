# Syncthing Role

Installs and configures [Syncthing](https://syncthing.net/) for file synchronization.

## Features

- Installs Syncthing from official APT repository
- Enables and starts Syncthing as a systemd service instance (running under the `syncthing_user` account)
- Optionally configures a workspace folder, and the remote device it is shared with
- Optionally turns off peer discovery, for a node its peer must dial
- Supports remote web UI access (optional)

## How it configures Syncthing

Through the REST API on `127.0.0.1:8384`, keyed by the API key Syncthing generates into `config.xml` — which the role reads once and never writes. `config.xml` is Syncthing's file: it re-serializes the whole document whenever it normalizes or migrates its configuration, dropping comments and reverting any edit made under the running process. A `blockinfile` marker survives exactly one deploy, and the second appends a duplicate.

Every write compares against the running configuration first and is skipped when it matches, so a converged Host reports no change — a `PUT` answers 200 whether or not it changed anything, so an unguarded write would report `changed` forever. Every field the folder write sends is compared before it is sent, `paused` included: a folder someone pauses in the web UI must not read as converged while replication is stopped.

Device IDs are compared dash-stripped and upper-cased. Syncthing canonicalizes an ID on parse, so the text pasted into `config.toml` is not necessarily the text `/rest/config` reports back, and a verbatim comparison would make a write that worked look like one that never converged.

`ansible.builtin.uri` declares no check mode, so the whole API block is skipped under `--check` — the reads would otherwise be skipped while the guards that dereference them still ran.

## Variables

| Variable                          | Default                            | Description                                                               |
| --------------------------------- | ---------------------------------- | ------------------------------------------------------------------------- |
| `syncthing_user`                  | `admin_user_name` / `ansible_user` | User to run Syncthing as                                                  |
| `syncthing_config_path`           | `~/.config/syncthing`              | Syncthing configuration directory (`config.xml`)                          |
| `syncthing_listen_all_interfaces` | `false`                            | `true` = bind the web UI to 0.0.0.0; `false` leaves the address untouched |
| `syncthing_discovery_enabled`     | unset                              | `false` = announce nowhere, no relays, no NAT mapping; unset = untouched  |
| `syncthing_configure_workspace`   | `false`                            | Auto-configure a workspace folder                                         |
| `syncthing_workspace_id`          | `""`                               | Folder ID in Syncthing                                                    |
| `syncthing_workspace_label`       | `""`                               | Folder label                                                              |
| `syncthing_workspace_path`        | `""`                               | Path to sync                                                              |
| `syncthing_device_id`             | `""`                               | Device ID to share folder with (optional)                                 |
| `syncthing_device_name`           | `""`                               | Display name for that device                                              |

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

### Peer-initiated replication

`syncthing_discovery_enabled: false` turns off global and local announce, relays, and NAT port mapping together. The node then announces nowhere and dials nobody: it is reachable only at its listen address, by a peer that already knows it. That is what the agent Host runs, because the tailnet ACL denies `tag:agent -> tag:trusted` and a node that cannot reach its peer should not spend outbound connections discovering that.

**Left unset, the role asserts nothing about discovery** — as it does for the workspace, and for the web-UI address. Syncthing's own web UI is a second writer for all three, so a role that enforced its defaults here would silently undo a Host hardened by hand on its next `apps.yml` deploy. One flag covers all four options because the agent Host's requirement is "announce nowhere"; split it when a Host needs LAN announce without public relays.

## Post-Install

After installation:

1. Access web UI (http://localhost:8384 or via SSH tunnel)
2. Note the device ID shown in web UI
3. Install Syncthing on desktop/mobile
4. Add VPS as remote device (use device ID)
5. Share your configured workspace folder between devices

## Tags

- `apps` - Application installation
- `sync` - File synchronization
- `syncthing` - Syncthing-specific tasks
