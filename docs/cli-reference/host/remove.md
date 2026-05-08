# auberge host remove

Remove a host from configuration

## Synopsis

```bash
auberge host remove [OPTIONS] [NAME]
# Alias: auberge h rm
```

## Description

Removes a host from the Auberge configuration. Prompts for confirmation unless --yes flag is used.

If `NAME` is omitted, you'll be prompted to select a host.

**Warning**: This only removes the host from Auberge configuration. It does not:

- Delete the VPS itself
- Remove SSH keys
- Clean up backups

## Arguments

| Argument | Description                               |
| -------- | ----------------------------------------- |
| NAME     | Host name to remove (omit to be prompted) |

## Options

| Option    | Description              | Default |
| --------- | ------------------------ | ------- |
| -y, --yes | Skip confirmation prompt | false   |

## Examples

```bash
# Remove with confirmation
auberge host remove myserver

# Interactive (prompts for host)
auberge host remove

# Skip confirmation (for scripts)
auberge host remove old-server --yes
```

## Interactive Session Example

```bash
$ auberge host remove myserver
Remove host 'myserver'? [y/N]: y
✓ Host 'myserver' removed
```
