# auberge host edit

Edit host configuration interactively

## Synopsis

```bash
auberge host edit [NAME]
# Alias: auberge h e
```

## Description

Opens an interactive prompt to edit an existing host's configuration. Allows updating address, user, port, tags, and description.

If `NAME` is omitted, you'll be prompted to select a host.

Note: Host name and SSH key cannot be changed through this command. To change the name, remove and re-add the host.

## Arguments

| Argument | Description                             |
| -------- | --------------------------------------- |
| NAME     | Host name to edit (omit to be prompted) |

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

# Interactive (prompts for host)
auberge host edit
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
