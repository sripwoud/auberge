# auberge dns delete

Delete the Cloudflare A record for a subdomain. Alias: `auberge d d`.

```bash
auberge dns delete [-s <SUBDOMAIN>] [--dry-run] [--production] [--yes]
```

Idempotent: reports success with "nothing to delete" if no A record exists. Only A records are considered — CNAME/AAAA/TXT records sharing the name are ignored.

## Options

| Option                | Description                     | Default         |
| --------------------- | ------------------------------- | --------------- |
| `-s, --subdomain`     | Subdomain (omit to be prompted) | Interactive     |
| `-n, --dry-run`       | Preview without deleting        | false           |
| `-o, --output FORMAT` | `human` or `json`               | `human`         |
| `-y, --yes`           | Skip confirmation               | false           |
| `-P, --production`    | Use production API              | false (sandbox) |

## Examples

```bash
auberge dns delete -s freshrss --dry-run
auberge dns delete -s freshrss
auberge dns delete -s calibre --production --yes   # CI / no prompts
```

## Gotchas

!> Production deletes require retyping the subdomain name to confirm. Sandbox uses a `[y/N]` prompt. Non-TTY without `--yes` exits without deleting (CI-safe).

<details>
<summary>JSON output schema</summary>

```json
{ "deleted": true, "fqdn": "freshrss.example.com", "production": false }
```

| Field        | Type    | Description                                  |
| ------------ | ------- | -------------------------------------------- |
| `deleted`    | boolean | `true` if removed; `false` if already absent |
| `fqdn`       | string  | Fully-qualified domain name targeted         |
| `production` | boolean | Whether the production API was used          |

JSON goes to stdout; human-format chrome goes to stderr. The `deleted` field distinguishes an actual deletion from an idempotent no-op — see [ADR-0004](../../../meta/adr/0004-cli-structured-output.md).

</details>
