# auberge dns status

Show DNS status and health

## Synopsis

```bash
auberge dns status [OPTIONS]
```

## Alias

`auberge d st`

## Description

Displays DNS configuration status, showing configured subdomains, active A records, and any missing records.

Checks for subdomains defined in environment variables against actual DNS records in Cloudflare.

## Options

| Option              | Description                           | Default |
| ------------------- | ------------------------------------- | ------- |
| -o, --output FORMAT | Output format (`human`, `json`)       | `human` |
| -P, --production    | Use production API (default: sandbox) | false   |

## Examples

```bash
# Show DNS status
auberge dns status

# Production API
auberge dns status --production
```

## Output Example

```
CLOUDFLARE DNS
DNS Status for example.com
----------------------------------------

Configured subdomains: blocky, calibre, freshrss, navidrome, baikal, webdav, yourls

Active A records: 7
  blocky -> 192.168.1.10
  calibre -> 192.168.1.10
  freshrss -> 192.168.1.10
  navidrome -> 192.168.1.10
  baikal -> 192.168.1.10
  webdav -> 192.168.1.10
  yourls -> 192.168.1.10

All configured subdomains have A records
```

## With Missing Records

```
CLOUDFLARE DNS
DNS Status for example.com
----------------------------------------

Configured subdomains: blocky, calibre, freshrss, navidrome

Active A records: 2
  blocky -> 192.168.1.10
  freshrss -> 192.168.1.10

Missing subdomains: calibre, navidrome
```

## JSON Output

```bash
auberge dns status --output json
```

```json
{
  "domain": "example.com",
  "configured_subdomains": ["blocky", "calibre", "freshrss", "navidrome"],
  "active_a_records": [
    { "name": "blocky", "ip": "192.168.1.10" },
    { "name": "freshrss", "ip": "192.168.1.10" }
  ],
  "missing_subdomains": ["calibre", "navidrome"]
}
```

JSON goes to stdout; human-format chrome (banners, info messages) goes to stderr.

**Schema**

| Field                   | Type     | Description                                                |
| ----------------------- | -------- | ---------------------------------------------------------- |
| domain                  | string   | Domain name from config                                    |
| configured_subdomains   | string[] | Subdomains discovered from `config.toml`                   |
| active_a_records        | object[] | A records present in Cloudflare                            |
| active_a_records[].name | string   | Subdomain label                                            |
| active_a_records[].ip   | string   | IP address the record points to                            |
| missing_subdomains      | string[] | Configured subdomains that have no A record (load-bearing) |

## Subdomain Discovery

Configured subdomains are discovered from `config.toml`:

- `blocky_subdomain`
- `freshrss_subdomain`
- `navidrome_subdomain`
- `baikal_subdomain`
- `webdav_subdomain`
- `yourls_subdomain`

Set with:

```bash
auberge config set freshrss_subdomain freshrss
auberge config set baikal_subdomain baikal
```

## Use Cases

**Health check**: Verify all app subdomains have DNS records

```bash
auberge dns status
```

**Pre-deployment**: Check DNS before running Ansible playbooks

```bash
auberge dns status
# If missing, run:
auberge dns set-all --host myserver
```

**Troubleshooting**: Identify missing or misconfigured records

```bash
auberge dns status
# Fix individual record:
auberge dns set --subdomain freshrss --ip 192.168.1.10
```

## Related Commands

- [auberge dns list](list.md) - List all DNS records
- [auberge dns set-all](set-all.md) - Fix missing subdomains
- [auberge dns migrate](migrate.md) - Update IPs for migration

## See Also

- [DNS Management](../../dns/README.md)
- [Deployment](../../deployment/README.md)
