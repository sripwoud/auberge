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

Discovers subdomain names from environment variables (SUBDOMAIN_FRESHRSS, SUBDOMAIN_RADICALE, etc.) and creates A records pointing to the selected host's IP.

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
| -o, --output FORMAT    | Output format: human, json, tsv             | human       |
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
auberge dns set-all --host myserver --subdomains freshrss,radicale

# Skip specific subdomains
auberge dns set-all --host myserver --skip calibre,yourls

# Strict mode (fail if env vars missing)
auberge dns set-all --host myserver --strict

# Continue on errors
auberge dns set-all --host myserver --continue-on-error
```

## Subdomain Discovery

Reads environment variables to discover subdomains:

- SUBDOMAIN_BLOCKY
- SUBDOMAIN_CALIBRE
- SUBDOMAIN_FRESHRSS
- SUBDOMAIN_NAVIDROME
- SUBDOMAIN_RADICALE
- SUBDOMAIN_WEBDAV
- SUBDOMAIN_YOURLS

Configure with mise:

```bash
mise set SUBDOMAIN_FRESHRSS="freshrss"
mise set SUBDOMAIN_RADICALE="radicale"
mise set SUBDOMAIN_CALIBRE="calibre"
mise set SUBDOMAIN_NAVIDROME="music"
mise set SUBDOMAIN_WEBDAV="files"
mise set SUBDOMAIN_YOURLS="s"
mise set SUBDOMAIN_BLOCKY="dns"
```

## Output Example

**Dry run**:

```
CLOUDFLARE DNS
DRY RUN - Would create the following A records:
  • freshrss.example.com → 192.168.1.10
  • radicale.example.com → 192.168.1.10
  • calibre.example.com → 192.168.1.10
  • music.example.com → 192.168.1.10

DRY RUN - No changes were made
```

**Execution**:

```
CLOUDFLARE DNS
Creating the following A records:
  • freshrss.example.com → 192.168.1.10
  • radicale.example.com → 192.168.1.10
  • calibre.example.com → 192.168.1.10
  • music.example.com → 192.168.1.10

Proceed? [y/N]: y

✓ Created freshrss.example.com
✓ Created radicale.example.com
✓ Created calibre.example.com
✓ Created music.example.com

Successfully created 4/4 A records pointing to 192.168.1.10
```

## Host Discovery

Auberge can automatically discover host IPs from inventory:

```bash
# Host "myserver" has ansible_host=192.168.1.10 in inventory
auberge dns set-all --host myserver
# Uses 192.168.1.10 automatically
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
auberge dns set-all --host myserver --subdomains freshrss,radicale
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
Failed radicale.example.com: API error
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
