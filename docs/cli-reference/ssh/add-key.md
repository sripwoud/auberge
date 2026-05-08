# auberge ssh add-key

Authorize an SSH public key on a remote host's `~/.ssh/authorized_keys`. Alias: `auberge ss ak`.

```bash
auberge ssh add-key [OPTIONS]
```

Requires existing SSH access to the target. Creates `~/.ssh` (700) if absent and sets `authorized_keys` to 600.

## Options

| Option                   | Description                      | Default     |
| ------------------------ | -------------------------------- | ----------- |
| `-H, --host HOST`        | Target host                      | Interactive |
| `-c, --connect-with KEY` | Private key to connect with      | Interactive |
| `-a, --authorize KEY`    | Public key (`.pub`) to authorize | Interactive |
| `-u, --user USER`        | Remote user                      | `ansible`   |
| `-y, --yes`              | Skip confirmation                | false       |

Both key options scan `~/.ssh/identities/` and `~/.ssh/` when prompting interactively.

## Examples

```bash
auberge ssh add-key --host myserver --authorize ~/.ssh/identities/ansible_myserver.pub
# key rotation: connect with old, authorize new
auberge ssh add-key --host myserver \
  --connect-with ~/.ssh/identities/ansible_myserver_old \
  --authorize ~/.ssh/identities/ansible_myserver.pub
# different remote user
auberge ssh add-key --host myserver --user deploy --authorize ~/.ssh/identities/deploy_myserver.pub
```

## Gotchas

- Adding a key that is already authorized is safe — SSH ignores duplicates.
- For a fresh VPS where only root has access, use `ssh-copy-id -i <key>.pub root@<ip>` first, then run `auberge ansible bootstrap` to create the ansible user before using this command.
