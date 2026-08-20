# SSH Keys

Three-tier resolution. Precedence: `--ssh-key` flag > `host.ssh_key` in `hosts.toml` > default derivation.

| Tier | Source                                           | Example                                                      |
| ---- | ------------------------------------------------ | ------------------------------------------------------------ |
| 1    | `--ssh-key` flag (one-off)                       | `auberge backup create --host my-vps --ssh-key /path/to/key` |
| 2    | `host.ssh_key` in `~/.config/auberge/hosts.toml` | `auberge host add my-vps --ssh-key ~/.ssh/custom`            |
| 3    | Derived: `~/.ssh/identities/{hostname}/{user}`   | `auberge ssh keygen --host my-vps --user ansible`            |

```toml
# ~/.config/auberge/hosts.toml
[[hosts]]
name = "my-vps"
address = "10.0.0.1"
user = "ansible"
port = 22
ssh_key = "~/.ssh/identities/custom_key"
```

?> When the derived path doesn't exist, commands fail and point at `auberge ssh keygen`. The only interactive picker lives in `auberge ssh add-key`, which scans `~/.ssh/`, `~/.ssh/identities/`, and `~/.ssh/identities/<host>/` to choose its connection and authorize keys; the selection is never saved to `hosts.toml`.

## Importing from `~/.ssh/config`

`auberge host add` (no args) detects an existing `~/.ssh/config` and offers to import host name, address, user, port, and `IdentityFile` from any host block.

## Common errors

| Error                                    | Fix                                                                                     |
| ---------------------------------------- | --------------------------------------------------------------------------------------- |
| `SSH key not found`                      | `auberge ssh keygen --host <host>` or fix the path with `auberge host edit <host>`      |
| `Configured SSH key not found` (warning) | Falls back to the derived path; fix the stale `ssh_key` with `auberge host edit <host>` |
| `No SSH private keys found`              | `auberge ssh keygen --host <host>` or copy keys into `~/.ssh/identities/`               |
| `Permissions 0644 are too open`          | `chmod 600 /path/to/key`                                                                |
