# auberge sync hermes

Sync `~/.config/hermes/config.yaml` to `~/.hermes/config.yaml` on a remote host and restart `hermes-gateway`. Alias: `auberge sy h`.

```bash
auberge sync hermes [OPTIONS]
```

Creates `~/.hermes/` on the remote if absent. Pass `--pull` to reverse direction (downloads remote → local, no service restart).

## Options

| Option              | Description                                      | Default                        |
| ------------------- | ------------------------------------------------ | ------------------------------ |
| `-H, --host HOST`   | Target host                                      | Interactive                    |
| `-s, --source PATH` | Local path (source on push, destination on pull) | `~/.config/hermes/config.yaml` |
| `-n, --dry-run`     | Preview without syncing                          | false                          |
| `-p, --pull`        | Pull remote config to local instead of pushing   | false                          |

## Examples

```bash
auberge sync hermes --host myserver
auberge sync hermes --host myserver --dry-run
auberge sync hermes --host myserver --pull
auberge sync hermes --host myserver --pull --source /tmp/hermes-config.yaml
```

## Gotchas

- `--dry-run` and `--pull` are mutually exclusive.
- Service restart failures: `ssh user@your-vps 'journalctl --user -u hermes-gateway -n 50'`.
