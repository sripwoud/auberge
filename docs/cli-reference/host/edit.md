# auberge host edit

Edit host configuration interactively

## Synopsis

```bash
auberge host edit <NAME>
```

## Description

Opens an interactive prompt to edit an existing host's configuration. Allows updating address, user, port, tags, and description.

Note: Host name and SSH key cannot be changed through this command. To change the name, remove and re-add the host.

## Arguments

| Argument | Description                  |
| -------- | ---------------------------- |
| NAME     | Host name to edit (required) |

## Editable Fields

- **Address**: Host address (IP or hostname)
- **User**: SSH user
- **Port**: SSH port
- **Tags**: Comma-separated tags
- **Description**: Host description

## Examples

```bash
# Edit host configuration
auberge host edit myserver
```

## Interactive Session Example

```
Host address [192.168.1.10]: 192.168.1.20
SSH user [ansible]: deploy
SSH port [2222]: 22
Tags (comma-separated) [production, web]: production, api
Description [Production web server]: Main API server
✓ Host 'myserver' updated
```

## Related Commands

- [auberge host show](show.md) - Show host details before editing
- [auberge host list](list.md) - List all hosts
- [auberge host add](add.md) - Add a new host
- [auberge host remove](remove.md) - Remove a host

## See Also

- [Host Configuration](../../configuration/hosts.md)
