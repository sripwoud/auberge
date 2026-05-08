# SSH Keys

Three-tier resolution. Precedence: `--ssh-key` flag > `host.ssh_key` in `hosts.toml` > default derivation.

| Tier | Source                                           | Example                                                      |
| ---- | ------------------------------------------------ | ------------------------------------------------------------ |
| 1    | `--ssh-key` flag (one-off)                       | `auberge backup create --host my-vps --ssh-key /path/to/key` |
| 2    | `host.ssh_key` in `~/.config/auberge/hosts.toml` | `auberge host add my-vps --ssh-key ~/.ssh/custom`            |
| 3    | Derived: `~/.ssh/identities/{user}_{hostname}`   | `auberge ssh keygen --host my-vps --user ansible`            |

```toml
# ~/.config/auberge/hosts.toml
[[hosts]]
name = "my-vps"
address = "10.0.0.1"
user = "ansible"
port = 22
ssh_key = "~/.ssh/identities/custom_key"
```

?> When the derived path doesn't exist, the CLI scans `~/.ssh/` and `~/.ssh/identities/` and offers an interactive picker. You can save the selection to `hosts.toml` so future runs skip the prompt.

## Importing from `~/.ssh/config`

`auberge host add` (no args) detects an existing `~/.ssh/config` and offers to import host name, address, user, port, and `IdentityFile` from any host block.

## Common errors

| Error                           | Fix                                                                                       |
| ------------------------------- | ----------------------------------------------------------------------------------------- |
| `No SSH keys found`             | `auberge ssh keygen --host <host>` or copy keys into `~/.ssh/identities/`                 |
| `Configured SSH key not found`  | Updates config or falls back to interactive picker. Edit with `auberge host edit <host>`. |
| `Permissions 0644 are too open` | `chmod 600 /path/to/key`                                                                  |
