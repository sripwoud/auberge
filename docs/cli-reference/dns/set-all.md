# auberge dns set-all

Batch create A records for all app subdomains

## Synopsis

```bash
auberge dns set-all [OPTIONS]
```

## Alias

`auberge d sa`

## Description

Interactively or automatically creates DNS A records for all configured app subdomains pointing to a host's IP address.

Discovers subdomain names from `config.toml` (e.g., `freshrss_subdomain`, `baikal_subdomain`) and creates A records pointing to the selected host's IP.

## Options

| Option                 | Description                                 | Default     |
| ---------------------- | ------------------------------------------- | ----------- |
| -H, --host HOST        | Target host (auberge, vibecoder, etc.)      | Interactive |
| -i, --ip IP            | Override IP address (conflicts with --host) | From host   |
| -n, --dry-run          | Preview without creating                    | false       |
| -y, --yes              | Skip confirmation prompt                    | false       |
| -s, --strict           | Fail if any subdomain env var missing       | false       |
| -S, --subdomains NAMES | Only process specific subdomains            | All         |
| --skip NAMES           | Skip specific subdomains                    | None        |
| -o, --output FORMAT    | Output format (`human`, `json`)             | `human`     |
| --continue-on-error    | Continue on errors instead of failing       | false       |
| -P, --production       | Use production API (default: sandbox)       | false       |

## Examples

```bash
# Interactive (select host, preview, confirm)
auberge dns set-all

# Specific host
auberge dns set-all --host myserver

# Override IP
auberge dns set-all --ip 192.168.1.10

# Dry run
auberge dns set-all --host myserver --dry-run

# Skip confirmation
auberge dns set-all --host myserver --yes

# Only specific subdomains
auberge dns set-all --host myserver --subdomains freshrss,baikal

# Skip specific subdomains
auberge dns set-all --host myserver --skip calibre,yourls

# Strict mode (fail if env vars missing)
auberge dns set-all --host myserver --strict

# Continue on errors
auberge dns set-all --host myserver --continue-on-error
```

## Subdomain Discovery

Reads `config.toml` to discover subdomains:

- `blocky_subdomain`
- `freshrss_subdomain`
- `navidrome_subdomain`
- `baikal_subdomain`
- `webdav_subdomain`
- `yourls_subdomain`

Configure with:

```bash
auberge config set freshrss_subdomain freshrss
auberge config set baikal_subdomain baikal
auberge config set navidrome_subdomain music
auberge config set webdav_subdomain files
auberge config set yourls_subdomain s
auberge config set blocky_subdomain dns
```

## Output Example

**Dry run**:

```
CLOUDFLARE DNS
DRY RUN - Would create the following A records:
  • freshrss.example.com → 192.168.1.10
  • baikal.example.com → 192.168.1.10
  • calibre.example.com → 192.168.1.10
  • music.example.com → 192.168.1.10

DRY RUN - No changes were made
```

**Execution**:

```
CLOUDFLARE DNS
Creating the following A records:
  • freshrss.example.com → 192.168.1.10
  • baikal.example.com → 192.168.1.10
  • calibre.example.com → 192.168.1.10
  • music.example.com → 192.168.1.10

Proceed? [y/N]: y

✓ Created freshrss.example.com
✓ Created baikal.example.com
✓ Created calibre.example.com
✓ Created music.example.com

Successfully created 4/4 A records pointing to 192.168.1.10
```

## JSON Output

```bash
auberge dns set-all --host myserver --output json
```

```json
{
  "created": [
    {
      "subdomain": "freshrss",
      "fqdn": "freshrss.example.com",
      "ip": "192.168.1.10",
      "success": true
    },
    {
      "subdomain": "baikal",
      "fqdn": "baikal.example.com",
      "ip": "192.168.1.10",
      "success": true
    }
  ],
  "skipped": [
    {"app": "bichon", "subdomain": "bichon", "reason": "tailnet_only"},
    {"app": "cockpit", "subdomain": "cockpit", "reason": "tailnet_only"},
    {"app": "paperless", "subdomain": "paperless", "reason": "tailnet_only"}
  ],
  "failed": []
}
```

JSON goes to stdout; human-format chrome (banners, info messages) goes to stderr.

**Schema — `created` / `failed` entries**

| Field     | Type    | Description                                                                                       |
| --------- | ------- | ------------------------------------------------------------------------------------------------- |
| subdomain | string  | Subdomain label                                                                                   |
| fqdn      | string  | Fully-qualified domain name                                                                       |
| ip        | string  | IP address the record points to                                                                   |
| success   | boolean | Whether the Cloudflare create/update succeeded (load-bearing)                                     |
| error     | string  | Error message when `success` is `false`; field is omitted when `success` is `true` (load-bearing) |

**Schema — `skipped` entries**

| Field     | Type   | Description                               |
| --------- | ------ | ----------------------------------------- |
| app       | string | Canonical app name (Playbook Meta stem)   |
| subdomain | string | Effective subdomain (operator-override-aware) |
| reason    | string | Always `"tailnet_only"` (matches meta key) |

Both `created` and `skipped` arrays are sorted alphabetically by app name for deterministic output.

## Host Discovery

Auberge can automatically discover host IPs from inventory:

```bash
# Host "myserver" has ansible_host=192.168.1.10 in inventory
auberge dns set-all --host myserver
# Uses 192.168.1.10 automatically
```

## Tailnet-only apps

Apps whose playbook meta declares `tailnet_only: true` (e.g., `bichon`, `cockpit`, and `paperless`) publish DNS exclusively via Blocky's `customDNS` map (ADR-0003). They do **not** receive public Cloudflare A records.

`dns set-all` handles them automatically by operator intent:

| How the app got there          | Behavior                                                                           |
| ------------------------------ | ---------------------------------------------------------------------------------- |
| Implicit discovery (no `--subdomains`) | Skipped silently; a grouped info line is emitted: `Skipping (tailnet-only — published via Blocky): <app1>, <app2>, …` |
| Explicit `--subdomains` target | Hard-error before any Cloudflare API call. Use `auberge deploy <app>` instead.    |

See [Tailnet-only Apps](../../dns/batch-operations.md#tailnet-only-apps) for the ADR-0003 context.

### Example — publish all Public Apps

```bash
# Publish all Public Apps; tailnet-only apps are skipped automatically
auberge dns set-all --host auberge --production
```

```
CLOUDFLARE DNS
Creating 7 A record(s), skipping 3 (tailnet-only):

To create:
  • rss.example.com → 82.223.116.111
  • calendrier.example.com → 82.223.116.111
  ...

Skipping (tailnet-only — published via Blocky):
  • bichon, cockpit, paperless
```

## Use Cases

**Initial setup** after deploying apps:

```bash
# 1. Deploy apps
auberge ansible run --host myserver

# 2. Create all DNS records
auberge dns set-all --host myserver
```

**New host setup**:

```bash
# Create all app subdomains for new server
auberge dns set-all --host newserver --dry-run
auberge dns set-all --host newserver
```

**Fix missing records**:

```bash
# After dns status shows missing subdomains
auberge dns set-all --host myserver
```

**Selective update**:

```bash
# Only create specific app records
auberge dns set-all --host myserver --subdomains freshrss,baikal
```

## Rate Limiting

Command includes 500ms delay between API calls to respect Cloudflare rate limits.

For many subdomains, execution may take a few seconds.

## Strict Mode

In strict mode (--strict):

- Fails if any SUBDOMAIN_* env var is missing
- Useful for CI/CD to ensure complete configuration
- Non-interactive mode

Without strict mode:

- Only creates records for defined subdomains
- Silently skips missing env vars

## Error Handling

**Default behavior**: Fail fast on first error

**With --continue-on-error**:

- Continues through all subdomains
- Reports final count of successes/failures
- Exits with error code if any failed

Example with errors:

```
✓ Created freshrss.example.com
Failed baikal.example.com: API error
✓ Created calibre.example.com

Successfully created 2/3 A records pointing to 192.168.1.10
Failed to create 1 records
```

## Related Commands

- [auberge dns status](status.md) - Check which records are missing
- [auberge dns set](set.md) - Create individual record
- [auberge dns list](list.md) - Verify created records

## See Also

- [DNS Management](../../dns/README.md)
- [Deployment](../../deployment/README.md)
- [Applications](../../applications/README.md)
