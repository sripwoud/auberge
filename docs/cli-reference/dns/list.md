# auberge dns list

List DNS records for your domain from Cloudflare. Alias: `auberge d l`.

```bash
auberge dns list [OPTIONS]
```

Requires `cloudflare_dns_api_token` and `domain` in `config.toml`. Displays A, AAAA, CNAME, MX, TXT, NS, and SRV records.

## Options

| Option                 | Description         | Default         |
| ---------------------- | ------------------- | --------------- |
| `-s, --subdomain NAME` | Filter by subdomain | All records     |
| `-o, --output FORMAT`  | `human` or `json`   | `human`         |
| `-P, --production`     | Use production API  | false (sandbox) |

## Examples

```bash
auberge dns list
auberge dns list --subdomain freshrss
auberge dns list --output json --production
```

<details>
<summary>JSON output schema</summary>

```json
[
  { "name": "@", "record_type": "A", "content": "192.168.1.10", "ttl": 1 }
]
```

| Field         | Type   | Description                    |
| ------------- | ------ | ------------------------------ |
| `name`        | string | Subdomain label (`@` for apex) |
| `record_type` | string | A, AAAA, CNAME, MX, …          |
| `content`     | string | IP, hostname, or text value    |
| `ttl`         | number | Time-to-live in seconds        |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
