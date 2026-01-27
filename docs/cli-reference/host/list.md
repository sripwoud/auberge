# auberge host list

List all configured hosts

## Synopsis

```bash
auberge host list [OPTIONS]
```

## Description

Displays a list of all configured hosts with their addresses, users, ports, and tags. Supports filtering by tags and multiple output formats.

## Options

| Option              | Description                      | Default         |
| ------------------- | -------------------------------- | --------------- |
| -t, --tags TAGS     | Filter by tags (comma-separated) | None (show all) |
| -o, --output FORMAT | Output format: table, json, yaml | table           |

## Output Formats

**Table** (default): Formatted table for terminal viewing

**JSON**: Machine-readable JSON array

**YAML**: YAML format for configuration

## Examples

```bash
# List all hosts (table format)
auberge host list

# Filter by single tag
auberge host list --tags production

# Filter by multiple tags
auberge host list --tags production,web

# JSON output
auberge host list --output json

# YAML output for backup/migration
auberge host list --output yaml > hosts-backup.yml
```

## Output Example

```
NAME      ADDRESS        USER     PORT  TAGS
myserver  192.168.1.10   ansible  2222  production, web
devbox    10.0.0.5       deploy   22    development
```

## Related Commands

- [auberge host add](add.md) - Add a new host
- [auberge host show](show.md) - Show detailed host information
- [auberge host edit](edit.md) - Edit host configuration
- [auberge host remove](remove.md) - Remove a host

## See Also

- [Host Configuration](../../configuration/hosts.md)
