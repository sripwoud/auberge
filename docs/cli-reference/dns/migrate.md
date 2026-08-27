# auberge dns migrate

Update all existing Cloudflare A records to a new IP. Alias: `auberge d m`.

```bash
auberge dns migrate --ip <IP> [OPTIONS]
```

## Options

| Option                | Description               | Default |
| --------------------- | ------------------------- | ------- |
| `-i, --ip IP`         | New IP address (required) | —       |
| `-n, --dry-run`       | Preview without updating  | `false` |
| `-o, --output FORMAT` | `human` or `json`         | `human` |

## Examples

```bash
auberge dns migrate --ip 10.0.0.5 --dry-run    # always preview first
auberge dns migrate --ip 10.0.0.5
auberge dns migrate --ip 10.0.0.5
```

## Gotchas

- Updates **only existing** A records. Doesn't create new ones.
- Skips records whose current IP is in CGNAT range `100.64.0.0/10` — a tailnet address is not a Host address, so repointing one would take a tailnet-only subdomain off the tailnet (ADR-0003). Each skip is reported: named under `Skipping (holds a tailnet address, not a Host address)` on `human`, and as a row in the `skipped` array on `json`.
- Only A records under the domain are candidates. The apex, records in other domains, and every non-A type (CNAME, AAAA, TXT, MX, NS, SRV) appear in neither array. `www` is not special-cased — it is untouched only when it is a CNAME, as it usually is.

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

`migrated` and `skipped` partition the records the run took as candidates, so a caller diffing against the zone can tell a record left behind on purpose from one the run never saw. `migrated` carries every in-scope record with the outcome of its write, failures included — read `success`, not membership.

```json
{
  "migrated": [
    {
      "subdomain": "rss",
      "old_ip": "203.0.113.10",
      "new_ip": "10.0.0.5",
      "success": true
    }
  ],
  "skipped": [
    { "subdomain": "bichon", "ip": "100.64.0.9", "reason": "tailnet_only" }
  ]
}
```

| Array      | Field       | Type    | Description                              |
| ---------- | ----------- | ------- | ---------------------------------------- |
| `migrated` | `subdomain` | string  | Subdomain label                          |
| `migrated` | `old_ip`    | string  | IP before migration                      |
| `migrated` | `new_ip`    | string  | IP after migration (the `--ip` argument) |
| `migrated` | `success`   | boolean | Cloudflare update succeeded              |
| `skipped`  | `subdomain` | string  | Subdomain label of the untouched record  |
| `skipped`  | `ip`        | string  | CGNAT address the record keeps           |
| `skipped`  | `reason`    | string  | Always `tailnet_only` (ADR-0003)         |

Rows appear in the order Cloudflare listed the zone; neither array is sorted.

!> `--dry-run` reports the same `skipped` rows as a real run, but every `migrated` row carries `success: true` without a write. `set-all` avoids this by discriminating on a top-level `outcome` and holding its untouched candidates in `planned` (ADR-0044); `migrate` has no such field yet, so on a dry run treat `migrated` as the plan, not the result.

Unlike [`set-all`](set-all.md)'s `skipped`, these rows carry no `app`: `migrate` reads the zone rather than the app roster, so a skipped subdomain need not name an app at all. `reason` is the vocabulary the two share.

JSON goes to stdout; banners and info messages go to stderr.
