# auberge headscale list-nodes

List nodes connected to a Headscale instance. Alias: `auberge hs ln`.

```bash
auberge headscale list-nodes [OPTIONS]
```

## Options

| Option                | Description            | Default     |
| --------------------- | ---------------------- | ----------- |
| `--host HOST`         | Host running headscale | Interactive |
| `-o, --output FORMAT` | `human` or `json`      | `human`     |

## Examples

```bash
auberge headscale list-nodes --host myserver
auberge headscale list-nodes --host myserver --output json
```

<details>
<summary>JSON output schema</summary>

```json
[{
  "id": 1,
  "name": "laptop",
  "user": "default",
  "ips": ["100.64.0.1", "fd7a:115c:a1e0::1"],
  "online": true,
  "last_seen": "2024-01-20T15:00:00Z"
}]
```

| Field       | Type     | Description            |
| ----------- | -------- | ---------------------- |
| `id`        | number   | Headscale node ID      |
| `name`      | string   | Node hostname          |
| `user`      | string   | Headscale user         |
| `ips`       | string[] | Assigned Tailscale IPs |
| `online`    | boolean  | Currently connected    |
| `last_seen` | string   | ISO 8601 UTC timestamp |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
