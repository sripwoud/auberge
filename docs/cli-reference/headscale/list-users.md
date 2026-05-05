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

| Option                | Description                     | Default            |
| --------------------- | ------------------------------- | ------------------ |
| `--host HOST`         | Target host running headscale   | Interactive prompt |
| `-o, --output FORMAT` | Output format (`human`, `json`) | `human`            |

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

## JSON Output

```bash
auberge headscale list-users --host myserver --output json
```

```json
[
  { "id": 1, "name": "default", "created": "2024-01-15T10:00:00Z" },
  { "id": 2, "name": "mobile", "created": "2024-01-20T14:30:00Z" }
]
```

JSON goes to stdout; human-format chrome (banners, info messages) goes to stderr.

**Schema**

| Field   | Type   | Description                       |
| ------- | ------ | --------------------------------- |
| id      | number | Headscale user ID                 |
| name    | string | User name                         |
| created | string | ISO 8601 creation timestamp (UTC) |

## Related Commands

- [auberge headscale add-user](add-user.md) - Create a user and generate a pre-auth key
- [auberge headscale list-nodes](list-nodes.md) - List connected nodes
- [auberge headscale remove-user](remove-user.md) - Remove a user

## See Also

- [Headscale](../../applications/networking/headscale.md)
