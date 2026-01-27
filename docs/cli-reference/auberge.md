# auberge

CLI for selfhost infrastructure management

## Synopsis

```bash
auberge [OPTIONS] <COMMAND>
```

## Description

Auberge is a comprehensive command-line tool for managing self-hosted infrastructure. It provides commands for managing VPS hosts, running Ansible playbooks, backing up and restoring application data, SSH key management, DNS management via Cloudflare, and file synchronization.

## Global Options

| Option        | Description                   |
| ------------- | ----------------------------- |
| -v, --verbose | Enable verbose output         |
| -q, --quiet   | Suppress non-essential output |
| -h, --help    | Print help information        |
| -V, --version | Print version information     |

## Commands

| Command                    | Alias | Description                             |
| -------------------------- | ----- | --------------------------------------- |
| [select](select/host.md)   | se    | Select hosts or playbooks interactively |
| [ansible](ansible/run.md)  | a     | Run ansible playbooks                   |
| [backup](backup/create.md) | b     | Backup and restore application data     |
| [host](host/add.md)        | h     | Manage VPS hosts                        |
| [ssh](ssh/keygen.md)       | ss    | SSH key management                      |
| [sync](sync/music.md)      | sy    | Sync files to remote hosts              |
| [dns](dns/list.md)         | d     | DNS management via Cloudflare           |

## Configuration

Auberge stores configuration in:

- **Hosts**: `~/.config/auberge/hosts.yml`
- **Backups**: `~/.local/share/auberge/backups/`
- **SSH keys**: `~/.ssh/identities/`

See [Configuration](../configuration/README.md) for details.

## Examples

```bash
# Add a new host
auberge host add myserver 192.168.1.10

# Run ansible playbook
auberge ansible run --host myserver

# Create backup
auberge backup create --host myserver

# List DNS records
auberge dns list
```

## See Also

- [Getting Started](../getting-started/README.md)
- [Configuration](../configuration/README.md)
- [Core Concepts](../core-concepts/README.md)
