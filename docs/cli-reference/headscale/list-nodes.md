# auberge headscale list-nodes

List connected Headscale nodes

## Synopsis

```bash
auberge headscale list-nodes [OPTIONS]
```

## Alias

`auberge hs ln`

## Description

Lists all nodes currently connected to the target Headscale instance, including their assigned IPs, online status, and the user they belong to.

## Options

| Option         | Description                     | Default            |
| -------------- | ------------------------------- | ------------------ |
| `--host HOST`  | Target host running headscale   | Interactive prompt |
| `-o, --output` | Output format (`json` or table) | table              |

## Examples

```bash
# List nodes on a host
auberge headscale list-nodes --host myserver

# Output as JSON
auberge headscale list-nodes --host myserver --output json
```

## Output Example

```
ID  HOSTNAME      USER     IP             ONLINE  LAST SEEN
1   laptop        default  100.64.0.1     yes     2024-01-20 15:00:00
2   phone         mobile   100.64.0.2     no      2024-01-19 08:00:00
3   home-server   default  100.64.0.3     yes     2024-01-20 15:00:00
```

## Related Commands

- [auberge headscale add-user](add-user.md) - Create a user and generate a pre-auth key
- [auberge headscale list-users](list-users.md) - List registered users
- [auberge headscale remove-user](remove-user.md) - Remove a user

## See Also

- [Headscale](../../applications/networking/headscale.md)
