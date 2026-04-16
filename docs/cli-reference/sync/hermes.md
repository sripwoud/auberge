# auberge sync hermes

Synchronize Hermes gateway config to remote host

## Synopsis

```bash
auberge sync hermes [OPTIONS]
```

## Alias

`auberge sy h`

## Description

Syncs `~/.config/hermes/config.yaml` to `~/.hermes/config.yaml` on the remote VPS and restarts `hermes-gateway`.

Creates `~/.hermes/` on the remote if it does not already exist.

Uses the SSH key configured for the target host in `~/.config/auberge/hosts.toml`.

Pass `--pull` to reverse the direction: downloads the remote config to the local source path without restarting the service.

## Options

| Option            | Description                                                              | Default                      |
| ----------------- | ------------------------------------------------------------------------ | ---------------------------- |
| -H, --host HOST   | Target host                                                              | Interactive selection        |
| -s, --source PATH | Config file path: source when pushing, destination when pulling          | ~/.config/hermes/config.yaml |
| -n, --dry-run     | Preview without syncing (incompatible with --pull)                       | false                        |
| -p, --pull        | Pull config from remote to local instead of pushing (no service restart) | false                        |

## Examples

```bash
# Sync to remote (interactive host selection)
auberge sync hermes

# Sync to specific host
auberge sync hermes --host myserver

# Dry run to preview changes
auberge sync hermes --host myserver --dry-run

# Pull remote config down to local
auberge sync hermes --host myserver --pull

# Pull to a custom local path
auberge sync hermes --host myserver --pull --source /tmp/hermes-config.yaml
```

## Sync Details

**Direction (push, default)**: `~/.config/hermes/config.yaml` → `~/.hermes/config.yaml` on remote

**Direction (pull)**: `~/.hermes/config.yaml` on remote → `~/.config/hermes/config.yaml` locally

After a push the remote `hermes-gateway` systemd user service is restarted automatically. Pull does not restart any service.

## Prerequisites

1. **SSH key configured** for the target host in `~/.config/auberge/hosts.toml`
2. **Hermes deployed** on the remote host:
   ```bash
   auberge deploy hermes
   ```
3. **Local config exists** at the source path (push) or remote config exists (pull)

## Troubleshooting

**SSH key not found**:

- Verify the host entry in `~/.config/auberge/hosts.toml` has the correct key path
- Generate a key if needed:
  ```bash
  auberge ssh keygen --host myserver
  ```

**Remote directory missing**:

- The command creates `~/.hermes/` automatically; if it still fails check remote disk space and user permissions

**Service fails to restart after sync**:

```bash
ssh user@your-vps 'journalctl --user -u hermes-gateway -n 50'
```

**--dry-run with --pull**:

- These flags are mutually exclusive; run without `--dry-run` when pulling

## Related Commands

- [auberge deploy hermes](../../applications/apps/hermes.md) - Deploy or update Hermes on the VPS
- [auberge config set](../config/overview.md) - Set Hermes config values

## See Also

- [Hermes Agent](../../applications/apps/hermes.md)
