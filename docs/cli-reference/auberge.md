# auberge

CLI for self-hosted infrastructure management.

```bash
auberge [GLOBAL OPTIONS] <COMMAND>
```

## Global options

| Option          | Description                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------------- |
| `-v, --verbose` | Stream subprocess output (kept on failure, dimmed on success)                                     |
| `-q, --quiet`   | Suppress chrome on stderr (errors and stdout data unchanged); mutually exclusive with `--verbose` |
| `--no-color`    | Disable colored output (or set `NO_COLOR` env var)                                                |
| `-h, --help`    | Print help                                                                                        |
| `-V, --version` | Print version                                                                                     |

## Commands

| Command                               | Alias | Purpose                             |
| ------------------------------------- | ----- | ----------------------------------- |
| [deploy](deploy.md)                   | `dp`  | Deploy apps with auto-hardening     |
| [ansible](ansible/run.md)             | `a`   | Run Ansible playbooks               |
| [backup](backup/create.md)            | `b`   | Backup / restore / push / prune     |
| [dns](dns/list.md)                    | `d`   | Cloudflare DNS management           |
| [host](host/add.md)                   | `h`   | Manage `hosts.toml`                 |
| [ssh](ssh/keygen.md)                  | `ss`  | SSH key generation and deployment   |
| [sync](sync/music.md)                 | `sy`  | rsync media to the VPS              |
| [headscale](headscale/add-user.md)    | `hs`  | Headscale users and nodes           |
| [bichon](bichon/reconcile-folders.md) | —     | Bichon folder reconciliation        |
| [config](config/overview.md)          | `c`   | Manage `config.toml`                |
| [select](select/host.md)              | `se`  | Interactive host / playbook pickers |

## Files

| Purpose  | Path                              |
| -------- | --------------------------------- |
| Hosts    | `~/.config/auberge/hosts.toml`    |
| Config   | `~/.config/auberge/config.toml`   |
| Backups  | `~/.local/share/auberge/backups/` |
| SSH keys | `~/.ssh/identities/`              |

See [Configuration](../configuration/hosts.md) for details on the config files.

## Examples

```bash
auberge host add my-vps 203.0.113.10
auberge deploy --all --host my-vps
auberge backup create --host my-vps
auberge dns list
```
