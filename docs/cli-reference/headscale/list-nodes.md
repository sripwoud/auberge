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
  "given_name": "laptop",
  "ip_addresses": ["100.64.0.1", "fd7a:115c:a1e0::1"],
  "user": { "name": "default" },
  "last_seen": { "seconds": 1705762800, "nanos": 0 },
  "online": true
}]
```

| Field          | Type     | Description                                          |
| -------------- | -------- | ---------------------------------------------------- |
| `id`           | number   | Headscale node ID                                    |
| `given_name`   | string   | Node hostname                                        |
| `user`         | object   | Owning user, `{ "name": … }`                         |
| `ip_addresses` | string[] | Assigned Tailscale IPs                               |
| `online`       | boolean  | Currently connected                                  |
| `last_seen`    | object   | `google.protobuf.Timestamp`, or `null` if never seen |

A subset of headscale's own node record, re-emitted under its field names. Fields headscale sends but auberge does not read — `machine_key`, `node_key`, `expiry`, `created_at`, `register_method`, `tags`, the route lists — are dropped, and `user` is reduced to its name. The human table renames and flattens what remains.

JSON goes to stdout; human-format chrome goes to stderr.

</details>
