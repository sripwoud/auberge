# auberge select host

Interactively select a host

## Synopsis

```bash
auberge select host [OPTIONS]
```

## Alias

`auberge se h`

## Description

Presents an interactive list of configured hosts for selection. Outputs the selected host name to stdout.

Useful for shell scripts and command substitution.

## Options

| Option            | Description           | Default   |
| ----------------- | --------------------- | --------- |
| -g, --group GROUP | Filter hosts by group | All hosts |

## Examples

```bash
# Select from all hosts
auberge select host

# Filter by group
auberge select host --group production

# Use in command substitution
HOST=$(auberge select host)
echo "Selected: $HOST"

# Use with other commands
auberge ansible run --host $(auberge select host)

# In shell scripts
#!/bin/bash
HOST=$(auberge select host --group production)
echo "Deploying to $HOST"
auberge ansible run --host "$HOST"
```

## Interactive Selection

```
? Select host
  myserver (192.168.1.10:22)
> devbox (10.0.0.5:2222)
  prod (203.0.113.5:22)

[↑↓ to move, Enter to select, Esc to cancel]
```

## Output

Outputs only the host name (no formatting):

```
myserver
```

Exit codes:

- 0: Host selected
- 1: No host selected (Esc pressed) or error

## Group Filtering

Filter hosts by group tag:

```bash
# Show only production hosts
auberge select host --group production

# Show only development hosts
auberge select host --group dev
```

Hosts are in a group if they have that tag configured:

```yaml
hosts:
  - name: myserver
    tags:
      - production
      - web
```

## Use Cases

**Interactive deployment**:

```bash
HOST=$(auberge select host)
auberge ansible run --host "$HOST" --playbook ansible/playbooks/apps.yml
```

**Shell script menu**:

```bash
#!/bin/bash
echo "Select backup target:"
TARGET=$(auberge select host --group production)

echo "Select backup source:"
SOURCE=$(auberge select host)

auberge backup restore latest --host "$TARGET" --from-host "$SOURCE"
```

**Quick SSH**:

```bash
# In shell alias or function
function assh() {
  HOST=$(auberge select host)
  if [ -n "$HOST" ]; then
    ssh "$(auberge host show "$HOST" -o json | jq -r '.user + "@" + .address')"
  fi
}
```

## Related Commands

- [auberge select playbook](playbook.md) - Select playbook interactively
- [auberge host list](../host/list.md) - List all hosts non-interactively

## See Also

- [Host Configuration](../../configuration/hosts.md)
- [Shell Integration](../../core-concepts/shell-integration.md)
