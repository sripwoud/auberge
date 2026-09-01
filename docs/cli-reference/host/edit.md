# auberge host edit

Edit host configuration interactively

## Synopsis

```bash
auberge host edit [NAME]
# Alias: auberge h e
```

## Description

Opens an interactive prompt to edit an existing host's configuration. Allows updating address, user, port, SSH key, tags, description, tailnet trust tier, and whether the CLI routes over the tailnet.

If `NAME` is omitted, you'll be prompted to select a host.

Note: Host name cannot be changed through this command — the name is a foreign key into the key directory layout, the remote hostname, and restic snapshot grouping. Use [`auberge host rename`](rename.md), which migrates all three.

## Arguments

| Argument | Description                             |
| -------- | --------------------------------------- |
| NAME     | Host name to edit (omit to be prompted) |

## Editable Fields

- **Address**: Host address (IP or hostname)
- **User**: SSH user
- **Port**: SSH port
- **SSH key**: Private key path, pre-filled with the current value (clear it to use the derived default `~/.ssh/identities/{hostname}/{user}`)
- **Tags**: Comma-separated tags
- **Description**: Host description
- **Tailnet trust tier**: picked from `(none)`, `trusted`, `data`, `agent`, `standby` (ADR-0055), current value preselected. A picker rather than a text field because the set is closed — a typo would otherwise surface as a `hosts.toml` parse failure on some later, unrelated command
- **Route over the tailnet address** (`prefer_tailnet`): asked only when the host has a cached `tailscale_ip`. Without one the prompt is skipped and says so, because the write would refuse the answer anyway and you would lose the seven you just gave. See [Tailnet Transport](configuration/tailnet-transport.md)

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
SSH key (empty for derived default): ~/.ssh/identities/myserver/deploy
Tags (comma-separated) [production, web]: production, api
Description [Production web server]: Main API server
? Tailnet trust tier ›
  (none)
  trusted
❯ data
  agent
  standby
Route over the tailnet address? [y/N]: y
✓ Host 'myserver' updated
```
