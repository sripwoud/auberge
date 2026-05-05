# auberge host list

List all configured hosts

## Synopsis

```bash
auberge host list [OPTIONS]
# Alias: auberge h l
```

## Description

Displays a list of all configured hosts with their addresses, users, ports, and tags. Supports filtering by tags and multiple output formats.

## Options

| Option              | Description                      | Default         |
| ------------------- | -------------------------------- | --------------- |
| -t, --tags TAGS     | Filter by tags (comma-separated) | None (show all) |
| -o, --output FORMAT | Output format (`human`, `json`)  | `human`         |

## Examples

```bash
# List all hosts
auberge host list

# Filter by single tag
auberge host list --tags production

# Filter by multiple tags
auberge host list --tags production,web

# JSON output
auberge host list --output json
```

## Output Example

```
NAME      ADDRESS        USER     PORT  TAGS
myserver  192.168.1.10   ansible  2222  production, web
devbox    10.0.0.5       deploy   22    development
```

## JSON Output

```bash
auberge host list --output json
```

```json
[
  {
    "name": "myserver",
    "address": "192.168.1.10",
    "user": "ansible",
    "port": 2222,
    "tags": ["production", "web"]
  },
  {
    "name": "devbox",
    "address": "10.0.0.5",
    "user": "deploy",
    "port": 22,
    "tags": ["development"]
  }
]
```

JSON goes to stdout; human-format chrome (banners, info messages) goes to stderr.

**Schema**

| Field   | Type     | Description               |
| ------- | -------- | ------------------------- |
| name    | string   | Host identifier           |
| address | string   | IP address or hostname    |
| user    | string   | SSH user                  |
| port    | number   | SSH port                  |
| tags    | string[] | Tags assigned to the host |

## Related Commands

- [auberge host add](add.md) - Add a new host
- [auberge host show](show.md) - Show detailed host information
- [auberge host edit](edit.md) - Edit host configuration
- [auberge host remove](remove.md) - Remove a host

## See Also

- [Host Configuration](../../configuration/hosts.md)
