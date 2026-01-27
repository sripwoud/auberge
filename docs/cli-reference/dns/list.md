# auberge dns list

List DNS records via Cloudflare

## Synopsis

```bash
auberge dns list [OPTIONS]
```

## Alias

`auberge d l`

## Description

Lists DNS records for your domain from Cloudflare. Displays A, AAAA, CNAME, MX, TXT, NS, and SRV records.

Requires CLOUDFLARE_API_TOKEN and CLOUDFLARE_ZONE_ID environment variables.

## Options

| Option               | Description                           | Default     |
| -------------------- | ------------------------------------- | ----------- |
| -s, --subdomain NAME | Filter by subdomain name              | All records |
| -P, --production     | Use production API (default: sandbox) | false       |

## Environment Variables

Required:

- `CLOUDFLARE_API_TOKEN`: Cloudflare API token with DNS read permissions
- `CLOUDFLARE_ZONE_ID`: Zone ID for your domain

Configure with mise:

```bash
mise set CLOUDFLARE_API_TOKEN="your-token-here"
mise set CLOUDFLARE_ZONE_ID="your-zone-id"
```

## Examples

```bash
# List all records
auberge dns list

# Filter by subdomain
auberge dns list --subdomain freshrss

# Production API
auberge dns list --production
```

## Output Example

```
CLOUDFLARE DNS
DNS Records for example.com

NAME                                     TYPE     CONTENT                  TTL
@                                        A        192.168.1.10             1
freshrss                                 A        192.168.1.10             1
radicale                                 A        192.168.1.10             1
www                                      CNAME    example.com              1
mail                                     MX       mail.example.com (10)    1
@                                        TXT      v=spf1 include:_...      1
```

## Record Types

| Type  | Description    | Content Format              |
| ----- | -------------- | --------------------------- |
| A     | IPv4 address   | 192.168.1.10                |
| AAAA  | IPv6 address   | 2001:db8::1                 |
| CNAME | Canonical name | example.com                 |
| MX    | Mail exchange  | mail.example.com (priority) |
| TXT   | Text record    | "v=spf1..."                 |
| NS    | Name server    | ns1.cloudflare.com          |
| SRV   | Service        | target.example.com          |

## Related Commands

- [auberge dns status](status.md) - Show DNS status
- [auberge dns set](set.md) - Set A record
- [auberge dns migrate](migrate.md) - Migrate all records to new IP
- [auberge dns set-all](set-all.md) - Batch create A records

## See Also

- [DNS Management](../../dns/README.md)
- [Cloudflare Setup](../../getting-started/cloudflare.md)
