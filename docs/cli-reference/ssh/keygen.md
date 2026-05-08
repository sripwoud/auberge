# auberge ssh keygen

Generate an ED25519 SSH key pair for host authentication. Alias: `auberge ss k`.

```bash
auberge ssh keygen [OPTIONS]
```

Keys are stored in `~/.ssh/identities/` as `{user}_{host}` (private) and `{user}_{host}.pub` (public), without a passphrase (required for Ansible and backup automation).

## Options

| Option            | Description                            | Default     |
| ----------------- | -------------------------------------- | ----------- |
| `-H, --host HOST` | Target host                            | Interactive |
| `-u, --user USER` | Key owner (`ansible` or your username) | `ansible`   |
| `-f, --force`     | Overwrite existing key                 | false       |

## Examples

```bash
auberge ssh keygen --host myserver
auberge ssh keygen --host myserver --user deploy
auberge ssh keygen --host myserver --force   # rotate key
```

## Gotchas

- Keys are stored in `~/.ssh/identities/` rather than `~/.ssh/` to avoid clutter and enable per-host/per-user management.
- Use `--force` to rotate; without it the command refuses to overwrite an existing key.
- After generating, authorize the key on the remote with `auberge ssh add-key`.
