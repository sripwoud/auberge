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

The human table's `TIER` column shows the ADR-0055 trust tier, `-` when unset, and `ROUTE` shows which address the CLI connects to — `public` or `tailnet` ([Tailnet Transport](configuration/tailnet-transport.md)). `ADDRESS` is always the declared public address, whatever `ROUTE` says. `--tags` filters on inventory groups, not on either column.

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
  "tags": ["production"],
  "tailnet_tag": "data",
  "prefer_tailnet": true
}]
```

| Field            | Type             | Description                                     |
| ---------------- | ---------------- | ----------------------------------------------- |
| `name`           | string           | Host identifier                                 |
| `address`        | string           | IP address or hostname                          |
| `user`           | string           | SSH user                                        |
| `port`           | number           | SSH port                                        |
| `tags`           | string[]         | Ansible inventory groups                        |
| `tailnet_tag`    | string \| absent | ADR-0055 trust tier; absent when unset          |
| `prefer_tailnet` | bool \| absent   | routes over `tailscale_ip`; absent when not set |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
