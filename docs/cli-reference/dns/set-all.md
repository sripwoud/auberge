# auberge dns set-all

Batch-create Cloudflare A records for all configured app subdomains. Alias: `auberge d sa`.

```bash
auberge dns set-all [OPTIONS]
```

## Options

| Option                   | Description                                              | Default     |
| ------------------------ | -------------------------------------------------------- | ----------- |
| `-H, --host HOST`        | Target host                                              | Interactive |
| `-i, --ip IP`            | Override IP (conflicts with `--host`)                    | From host   |
| `-n, --dry-run`          | Preview without creating                                 | `false`     |
| `-y, --yes`              | Skip confirmation                                        | `false`     |
| `-s, --strict`           | Fail if any subdomain env var missing                    | `false`     |
| `-S, --subdomains NAMES` | Process only these subdomains                            | All         |
| `--skip NAMES`           | Exclude these subdomains                                 | None        |
| `--continue-on-error`    | Continue past errors                                     | `false`     |
| `-o, --output FORMAT`    | `human` or `json`                                        | `human`     |
| `-P, --production`       | Accepted and ignored; every call uses the production API | n/a         |

## Examples

```bash
auberge dns set-all                                              # interactive
auberge dns set-all --host my-vps --dry-run
auberge dns set-all --host my-vps --subdomains freshrss,baikal
auberge dns set-all --host my-vps --skip calibre,yourls --yes
auberge dns set-all --host my-vps --strict                       # CI: fail on missing config
```

## Subdomain discovery

Reads `*_subdomain` keys from `config.toml` (e.g. `freshrss_subdomain`, `baikal_subdomain`). Set them with `auberge config set <app>_subdomain <name>`.

## Tailnet-only apps

Apps whose playbook meta declares `tailnet_only: true` (currently `bichon`, `cockpit`, `paperless`) publish DNS via Blocky's `customDNS` map (ADR-0003) — never via Cloudflare.

| Source                                             | Behavior                                                                       |
| -------------------------------------------------- | ------------------------------------------------------------------------------ |
| Implicit (no `--subdomains`)                       | Skipped silently with `Skipping (tailnet-only — published via Blocky): <apps>` |
| Explicit (`--subdomains` names a tailnet-only app) | Hard-error before any record is written. Use `auberge deploy <app>` instead.   |

!> A 500 ms delay is inserted between API calls to respect Cloudflare rate limits.

## Exit codes

Follow the Backup Verdict convention, so a script can branch on which of the three happened:

| Code | Meaning                                                                                                        |
| ---- | -------------------------------------------------------------------------------------------------------------- |
| `0`  | Every planned record written — including a run with nothing to do, a `--dry-run`, and a cancelled confirmation |
| `1`  | At least one write failed; the failures are in the `failed` array                                              |
| `2`  | Operational error — no `--host`/`--ip`, host absent from inventory, a tailnet-only app named in `--subdomains` |

Under `--output json`, every path the run can end on emits a body (ADR-0044) — a failed write before exiting `1`, a dry run, and a declined confirmation alike:

```bash
auberge dns set-all --host my-vps --yes --output json --continue-on-error > records.json || echo "some records failed"
auberge dns set-all --host my-vps --dry-run --output json | jq '.planned'
```

<details>
<summary>JSON output schema</summary>

One shape on every path. `outcome` says what the run did with its plan; `planned` always holds the full plan.

```json
{
  "outcome": "applied",
  "planned": [
    {
      "app": "freshrss",
      "subdomain": "rss",
      "fqdn": "rss.example.com",
      "ip": "203.0.113.10"
    }
  ],
  "created": [
    {
      "subdomain": "rss",
      "fqdn": "rss.example.com",
      "ip": "203.0.113.10",
      "success": true
    }
  ],
  "skipped": [
    { "app": "bichon", "subdomain": "bichon", "reason": "tailnet_only" }
  ],
  "failed": []
}
```

| Field              | Contents                                       | Description                                                                               |
| ------------------ | ---------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `outcome`          | `"applied"` \| `"dry_run"` \| `"cancelled"`    | What the run did with its plan — branch here, not on array emptiness                      |
| `planned`          | `app`, `subdomain`, `fqdn`, `ip`               | The full plan with effective IPs, on every outcome — the denominator for the arrays below |
| `created`/`failed` | `subdomain`, `fqdn`, `ip`, `success`, `error?` | Operation result per app; both empty unless `outcome` is `applied`                        |
| `skipped`          | `app`, `subdomain`, `reason`                   | Always `reason: "tailnet_only"`                                                           |

- `outcome: "applied"` — the plan ran; `created` and `failed` partition `planned`.
- `outcome: "dry_run"` — nothing was written; the plan is under `planned`, and `created`/`failed` are empty.
- `outcome: "cancelled"` — the `Proceed?` confirmation was declined; same body as a dry run. A non-interactive caller that forgets `--yes` lands here (the prompt refuses off-terminal), so a non-empty `planned` with `outcome: "cancelled"` is the "you forgot `--yes`" signal.

All arrays are sorted alphabetically by app name. JSON to stdout; chrome to stderr.

</details>
