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
[
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
  },
  {
    "subdomain": "calibre",
    "fqdn": "calibre.example.com",
    "ip": "192.168.1.10",
    "success": false,
    "error": "API rate limit exceeded"
  }
]
```

JSON goes to stdout; human-format chrome (banners, info messages) goes to stderr.

**Schema**

| Field     | Type    | Description                                                                                       |
| --------- | ------- | ------------------------------------------------------------------------------------------------- |
| subdomain | string  | Subdomain label                                                                                   |
| fqdn      | string  | Fully-qualified domain name                                                                       |
| ip        | string  | IP address the record points to                                                                   |
| success   | boolean | Whether the Cloudflare create/update succeeded (load-bearing)                                     |
| error     | string  | Error message when `success` is `false`; field is omitted when `success` is `true` (load-bearing) |

## Host Discovery

Auberge can automatically discover host IPs from inventory:

```bash
# Host "myserver" has ansible_host=192.168.1.10 in inventory
auberge dns set-all --host myserver
# Uses 192.168.1.10 automatically
```

## Tailnet-only apps

Apps whose playbook meta declares `tailnet_only: true` (currently `bichon` and `paperless`) need to point at the host's Tailscale CGNAT IP, not its public IP — Caddy binds those vhosts only to the Tailnet interface.

The IP is resolved per-subdomain in this order:

1. `<app>_tailscale_ip` in `config.toml` (explicit per-app override)
2. `host.tailscale_ip` from `hosts.toml` (cached via [`auberge host detect-tailscale-ip`](../host/detect-tailscale-ip.md), used only when `--host` is passed)
3. The host's public IP (default for public apps)

If a tailnet-only app has no resolvable Tailscale IP, `dns set-all` bails — silently pointing such DNS at the public IP would make the service unreachable.

See [Tailnet-only Subdomains](../../dns/batch-operations.md#tailnet-only-subdomains) for the full pattern.

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
