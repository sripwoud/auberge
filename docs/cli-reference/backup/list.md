# auberge backup list

List available local backups. Alias: `auberge b ls`.

```bash
auberge backup list [OPTIONS]
```

## Options

| Option                | Description           | Default   |
| --------------------- | --------------------- | --------- |
| `-H, --host HOST`     | Filter by host        | All hosts |
| `-a, --app APP`       | Filter by application | All apps  |
| `-o, --output FORMAT` | `human` or `json`     | `human`   |

## Examples

```bash
auberge backup list
auberge backup list --host myserver --app freshrss
auberge backup list --output json
```

<details>
<summary>JSON output schema</summary>

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

| Field        | Type   | Description                           |
| ------------ | ------ | ------------------------------------- |
| `host`       | string | Host the backup was taken from        |
| `app`        | string | Application name                      |
| `timestamp`  | string | `YYYY-MM-DD_HH-MM-SS`                 |
| `path`       | string | Absolute path to the backup directory |
| `size_bytes` | number | Backup size in bytes                  |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
