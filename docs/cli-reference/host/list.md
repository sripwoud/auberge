# auberge host list

List all configured hosts. Alias: `auberge h l`.

```bash
auberge host list [OPTIONS]
```

## Options

| Option                | Description                      | Default   |
| --------------------- | -------------------------------- | --------- |
| `-t, --tags TAGS`     | Filter by tags (comma-separated) | All hosts |
| `-o, --output FORMAT` | `human` or `json`                | `human`   |

## Examples

```bash
auberge host list
auberge host list --tags production,web
auberge host list --output json
```

<details>
<summary>JSON output schema</summary>

```json
[{
  "name": "myserver",
  "address": "192.168.1.10",
  "user": "ansible",
  "port": 2222,
  "tags": ["production"]
}]
```

| Field     | Type     | Description               |
| --------- | -------- | ------------------------- |
| `name`    | string   | Host identifier           |
| `address` | string   | IP address or hostname    |
| `user`    | string   | SSH user                  |
| `port`    | number   | SSH port                  |
| `tags`    | string[] | Tags assigned to the host |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
