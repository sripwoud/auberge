# auberge dns migrate

Update all existing Cloudflare A records to a new IP. Alias: `auberge d m`.

```bash
auberge dns migrate --ip <IP> [OPTIONS]
```

## Options

| Option                | Description                           | Default |
| --------------------- | ------------------------------------- | ------- |
| `-i, --ip IP`         | New IP address (required)             | —       |
| `-n, --dry-run`       | Preview without updating              | `false` |
| `-o, --output FORMAT` | `human` or `json`                     | `human` |
| `-P, --production`    | Use production API (default: sandbox) | `false` |

## Examples

```bash
auberge dns migrate --ip 10.0.0.5 --dry-run    # always preview first
auberge dns migrate --ip 10.0.0.5
auberge dns migrate --ip 10.0.0.5 --production
```

## Gotchas

- Updates **only existing** A records. Doesn't create new ones.
- Skips records whose current IP is in CGNAT range `100.64.0.0/10` — protects tailnet-only subdomains (per ADR-0003) from accidental migration to a public IP.
- Apex domain and `www` are not touched (often CNAMEs).

## VPS migration workflow

```bash
auberge ansible bootstrap new-vps --ip 10.0.0.5
auberge ansible run --host new-vps
auberge backup restore latest --from-host old-vps --host new-vps
auberge dns migrate --ip 10.0.0.5 --dry-run
auberge dns migrate --ip 10.0.0.5
dig +short cal.example.com    # verify
```

## JSON output

```json
[{
  "subdomain": "rss",
  "old_ip": "203.0.113.10",
  "new_ip": "10.0.0.5",
  "success": true
}]
```

| Field       | Type    | Description                              |
| ----------- | ------- | ---------------------------------------- |
| `subdomain` | string  | Subdomain label                          |
| `old_ip`    | string  | IP before migration                      |
| `new_ip`    | string  | IP after migration (the `--ip` argument) |
| `success`   | boolean | Cloudflare update succeeded              |

JSON goes to stdout; banners and info messages go to stderr.
