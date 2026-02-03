# auberge backup list

List available backups

## Synopsis

```bash
auberge backup list [OPTIONS]
```

## Alias

`auberge b ls`

## Description

Lists all available backups with filtering options. Shows host, application, timestamp, and size information.

## Options

| Option              | Description                      | Default   |
| ------------------- | -------------------------------- | --------- |
| -H, --host HOST     | Filter by host                   | All hosts |
| -a, --app APP       | Filter by application            | All apps  |
| -f, --format FORMAT | Output format: table, json, yaml | table     |

## Examples

```bash
# List all backups
auberge backup list

# Filter by host
auberge backup list --host myserver

# Filter by application
auberge backup list --app baikal

# Combine filters
auberge backup list --host myserver --app freshrss

# JSON output
auberge backup list --format json

# YAML output
auberge backup list --format yaml
```

## Output Formats

**Table** (default):

```
HOST       APP        TIMESTAMP            SIZE
myserver   baikal   2024-01-27_14-30-00  2.3 MB
myserver   freshrss   2024-01-27_14-30-00  15.7 MB
myserver   navidrome  2024-01-27_14-30-00  45.2 KB
devbox     baikal   2024-01-26_10-15-30  1.8 MB

Total: 4 backup(s)
```

**JSON**:

```json
[
  {
    "host": "myserver",
    "app": "baikal",
    "timestamp": "2024-01-27_14-30-00",
    "path": "/home/user/.local/share/auberge/backups/myserver/2024-01-27_14-30-00/baikal",
    "size_bytes": 2411520
  }
]
```

**YAML**:

```yaml
- host: myserver
  app: baikal
  timestamp: 2024-01-27_14-30-00
  path: /home/user/.local/share/auberge/backups/myserver/2024-01-27_14-30-00/baikal
  size_bytes: 2411520
```

## Backup Organization

Backups are stored in timestamp-first structure:

```
~/.local/share/auberge/backups/
└── myserver/
    ├── 2024-01-27_14-30-00/
    │   ├── baikal/
    │   ├── freshrss/
    │   └── navidrome/
    └── 2024-01-26_10-15-30/
        └── baikal/
```

## Related Commands

- [auberge backup create](create.md) - Create new backup
- [auberge backup restore](restore.md) - Restore from backup

## See Also

- [Backup and Restore](../../backup-restore/README.md)
