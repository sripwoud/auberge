# auberge dns set-all

Batch-create Cloudflare A records for all configured app subdomains. Alias: `auberge d sa`.

```bash
auberge dns set-all [OPTIONS]
```

## Options

| Option                   | Description                           | Default     |
| ------------------------ | ------------------------------------- | ----------- |
| `-H, --host HOST`        | Target host                           | Interactive |
| `-i, --ip IP`            | Override IP (conflicts with `--host`) | From host   |
| `-n, --dry-run`          | Preview without creating              | `false`     |
| `-y, --yes`              | Skip confirmation                     | `false`     |
| `-s, --strict`           | Fail if any subdomain env var missing | `false`     |
| `-S, --subdomains NAMES` | Process only these subdomains         | All         |
| `--skip NAMES`           | Exclude these subdomains              | None        |
| `--continue-on-error`    | Continue past errors                  | `false`     |
| `-o, --output FORMAT`    | `human` or `json`                     | `human`     |
| `-P, --production`       | Use production API (default: sandbox) | `false`     |

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
| Explicit (`--subdomains` names a tailnet-only app) | Hard-error before any API call. Use `auberge deploy <app>` instead.            |

!> A 500 ms delay is inserted between API calls to respect Cloudflare rate limits.

<details>
<summary>JSON output schema</summary>

```json
{
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

| Array              | Field                                          | Type   | Description                     |
| ------------------ | ---------------------------------------------- | ------ | ------------------------------- |
| `created`/`failed` | `subdomain`, `fqdn`, `ip`, `success`, `error?` | mixed  | Operation result per app        |
| `skipped`          | `app`, `subdomain`, `reason`                   | string | Always `reason: "tailnet_only"` |

Both arrays are sorted alphabetically by app name. JSON to stdout; chrome to stderr.

</details>
