# auberge host remove

Remove a host from configuration

## Synopsis

```bash
auberge host remove [OPTIONS] <NAME>
```

## Description

Removes a host from the Auberge configuration. Prompts for confirmation unless --yes flag is used.

**Warning**: This only removes the host from Auberge configuration. It does not:

- Delete the VPS itself
- Remove SSH keys
- Clean up backups

## Arguments

| Argument | Description                    |
| -------- | ------------------------------ |
| NAME     | Host name to remove (required) |

## Options

| Option    | Description              | Default |
| --------- | ------------------------ | ------- |
| -y, --yes | Skip confirmation prompt | false   |

## Examples

```bash
# Remove with confirmation
auberge host remove myserver

# Skip confirmation (for scripts)
auberge host remove old-server --yes
```

## Interactive Session Example

```bash
$ auberge host remove myserver
Remove host 'myserver'? [y/N]: y
✓ Host 'myserver' removed
```

## Related Commands

- [auberge host list](list.md) - List all hosts
- [auberge host add](add.md) - Add a new host
- [auberge host show](show.md) - Show host details

## See Also

- [Host Configuration](../../configuration/hosts.md)
