# auberge headscale list-users

List registered Headscale users

## Synopsis

```bash
auberge headscale list-users [OPTIONS]
```

## Alias

`auberge hs lu`

## Description

Lists all users registered on the target Headscale instance.

## Options

| Option         | Description                     | Default            |
| -------------- | ------------------------------- | ------------------ |
| `--host HOST`  | Target host running headscale   | Interactive prompt |
| `-o, --output` | Output format (`json` or table) | table              |

## Examples

```bash
# List users on a host
auberge headscale list-users --host myserver

# Output as JSON
auberge headscale list-users --host myserver --output json
```

## Output Example

```
ID  NAME     CREATED
1   default  2024-01-15 10:00:00 UTC
2   mobile   2024-01-20 14:30:00 UTC
```

## Related Commands

- [auberge headscale add-user](add-user.md) - Create a user and generate a pre-auth key
- [auberge headscale list-nodes](list-nodes.md) - List connected nodes
- [auberge headscale remove-user](remove-user.md) - Remove a user

## See Also

- [Headscale](../../applications/networking/headscale.md)
