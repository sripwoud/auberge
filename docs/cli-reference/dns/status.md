# auberge dns status

Show which configured subdomains have active Cloudflare A records and which are missing. Alias: `auberge d st`.

```bash
auberge dns status [OPTIONS]
```

Configured subdomains are discovered from `*_subdomain` keys in `config.toml` (e.g. `freshrss_subdomain`, `baikal_subdomain`).

## Options

| Option                | Description        | Default         |
| --------------------- | ------------------ | --------------- |
| `-o, --output FORMAT` | `human` or `json`  | `human`         |
| `-P, --production`    | Use production API | false (sandbox) |

## Examples

```bash
auberge dns status
auberge dns status --production
auberge dns status --output json
```

## Gotchas

- Missing subdomains are surfaced in both human and JSON output — the `missing_subdomains` field is the actionable signal.
- Fix missing records with `auberge dns set --subdomain <name> --ip <ip>` or bulk-set with `auberge dns set-all --host myserver`.

<details>
<summary>JSON output schema</summary>

```json
{
  "domain": "example.com",
  "configured_subdomains": ["blocky", "freshrss"],
  "active_a_records": [{ "name": "blocky", "ip": "192.168.1.10" }],
  "missing_subdomains": ["freshrss"]
}
```

| Field                     | Type     | Description                            |
| ------------------------- | -------- | -------------------------------------- |
| `domain`                  | string   | Domain from config                     |
| `configured_subdomains`   | string[] | Subdomains from `config.toml`          |
| `active_a_records`        | object[] | A records present in Cloudflare        |
| `active_a_records[].name` | string   | Subdomain label                        |
| `active_a_records[].ip`   | string   | IP the record points to                |
| `missing_subdomains`      | string[] | Configured subdomains with no A record |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
