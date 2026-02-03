# auberge dns set

Set A record for a subdomain

## Synopsis

```bash
auberge dns set --subdomain <NAME> --ip <IP>
```

## Alias

`auberge d s`

## Description

Creates or updates an A record for a subdomain pointing to an IP address.

If the record exists, it will be updated. If it doesn't exist, it will be created.

## Options

| Option               | Description                           | Required |
| -------------------- | ------------------------------------- | -------- |
| -s, --subdomain NAME | Subdomain name                        | Yes      |
| -i, --ip IP          | IP address                            | Yes      |
| -P, --production     | Use production API (default: sandbox) | No       |

## Examples

```bash
# Set A record
auberge dns set --subdomain freshrss --ip 192.168.1.10

# Update existing record
auberge dns set --subdomain baikal --ip 10.0.0.5

# Production API
auberge dns set --subdomain calibre --ip 192.168.1.10 --production
```

## Output Example

```
CLOUDFLARE DNS
Setting A record: freshrss.example.com -> 192.168.1.10
✓ A record set successfully
```

## Behavior

**Record exists**: Updates IP address
**Record doesn't exist**: Creates new A record
**TTL**: Set to 1 (automatic)
**Proxied**: Not proxied (DNS only)

## DNS Propagation

Changes are immediate in Cloudflare but DNS propagation can take:

- **Cloudflare nameservers**: Instant
- **Recursive resolvers**: Up to TTL value (typically 1 second with TTL=1)
- **Client caches**: Varies by OS/application

Verify with:

```bash
dig freshrss.example.com
nslookup freshrss.example.com
```

## IP Validation

The command validates IP format:

- IPv4: 192.168.1.10 (octets 0-255)
- IPv6: 2001:db8::1

Invalid IPs are rejected:

```bash
$ auberge dns set --subdomain test --ip 999.999.999.999
Error: Invalid IP format: 999.999.999.999
```

## Use Cases

**Single subdomain setup**:

```bash
auberge dns set --subdomain freshrss --ip 192.168.1.10
```

**IP change after migration**:

```bash
auberge dns set --subdomain baikal --ip 10.0.0.5
```

**Fix missing record**:

```bash
# After dns status shows missing
auberge dns set --subdomain calibre --ip 192.168.1.10
```

## Batch Operations

For multiple subdomains, use:

```bash
auberge dns set-all --host myserver
```

Or script individual sets:

```bash
for sub in freshrss baikal calibre; do
  auberge dns set --subdomain $sub --ip 192.168.1.10
done
```

## Related Commands

- [auberge dns list](list.md) - List all records
- [auberge dns set-all](set-all.md) - Batch create records
- [auberge dns migrate](migrate.md) - Migrate all records

## See Also

- [DNS Management](../../dns/README.md)
- [Cloudflare Setup](../../getting-started/cloudflare.md)
