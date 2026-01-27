# Managing DNS Records

Guide to managing individual DNS records via Auberge CLI.

## Overview

Auberge provides commands to:

- List existing DNS records
- Create/update A records for subdomains
- Check DNS health and status

**Note:** These commands work with Cloudflare API. See [Cloudflare Setup](dns/cloudflare-setup.md) for configuration.

## List Records

View all DNS records for your domain:

```bash
auberge dns list
```

**Example output:**

```
CLOUDFLARE DNS

DNS Records for example.com
NAME                                     TYPE     CONTENT                  TTL
--------------------------------------------------------------------------------
example.com                              A        194.164.53.11            300
www.example.com                          CNAME    example.com              300
cal.example.com                          A        194.164.53.11            300
rss.example.com                          A        194.164.53.11            300
music.example.com                        A        194.164.53.11            300
```

### Filter by Subdomain

```bash
auberge dns list --subdomain cal
```

**Output:**

```
NAME                                     TYPE     CONTENT                  TTL
--------------------------------------------------------------------------------
cal.example.com                          A        194.164.53.11            300
```

### Alias

Short form:

```bash
auberge dns l
auberge dns l --subdomain rss
```

## DNS Status

Check health and missing records:

```bash
auberge dns status
```

**Example output:**

```
CLOUDFLARE DNS

DNS Status for example.com
----------------------------------------

Configured subdomains: dns, lire, rss, musique, calendrier, webdav, url

Active A records: 5
  cal.example.com -> 194.164.53.11
  rss.example.com -> 194.164.53.11
  music.example.com -> 194.164.53.11
  files.example.com -> 194.164.53.11
  url.example.com -> 194.164.53.11

Missing subdomains: dns, lire
```

**Use case:** Quickly see which subdomains need A records created.

### Alias

```bash
auberge dns st
```

## Set A Record

Create or update a single A record:

```bash
auberge dns set --subdomain cal --ip 194.164.53.11
```

**Example output:**

```
CLOUDFLARE DNS

Setting A record: cal.example.com -> 194.164.53.11
✓ A record set successfully
```

### Behavior

- **If record exists:** Updates IP address
- **If record doesn't exist:** Creates new A record
- **TTL:** Uses default (300 seconds)

### Alias

Short form:

```bash
auberge dns s --subdomain rss --ip 10.0.0.1
```

## Common Use Cases

### Initial DNS Setup

Set up all records for a new deployment:

```bash
# Get VPS IP
AUBERGE_IP="194.164.53.11"

# Set each subdomain
auberge dns set --subdomain dns --ip $AUBERGE_IP
auberge dns set --subdomain cal --ip $AUBERGE_IP
auberge dns set --subdomain rss --ip $AUBERGE_IP
auberge dns set --subdomain music --ip $AUBERGE_IP
auberge dns set --subdomain books --ip $AUBERGE_IP
auberge dns set --subdomain files --ip $AUBERGE_IP
auberge dns set --subdomain url --ip $AUBERGE_IP
```

**Better alternative:** Use `dns set-all` for batch operations:

```bash
auberge dns set-all --host auberge
```

See [Batch Operations](dns/batch-operations.md).

### Update Single Subdomain

After adding a new app:

```bash
# Add subdomain to mise.toml
echo 'MYAPP_SUBDOMAIN = "myapp"' >> mise.toml

# Create DNS record
auberge dns set --subdomain myapp --ip 194.164.53.11
```

### Move Single Service to Different IP

Migrate one subdomain to new VPS:

```bash
# Old IP: 194.164.53.11
# New IP: 10.0.0.1

auberge dns set --subdomain cal --ip 10.0.0.1
```

### Verify Changes

```bash
# Set record
auberge dns set --subdomain test --ip 10.0.0.1

# Verify it was created
auberge dns list --subdomain test

# Check via dig
dig +short test.example.com
# Should return: 10.0.0.1
```

## Record Types

### A Records (IPv4)

Current commands only support A records:

```bash
auberge dns set --subdomain www --ip 194.164.53.11
```

Creates:

```
www.example.com  A  194.164.53.11
```

### AAAA Records (IPv6)

Not currently supported via CLI.

**Workaround:** Create manually in Cloudflare Dashboard.

### CNAME Records

Not supported via CLI.

**Workaround:** Create manually in Cloudflare Dashboard.

**Example use case:**

```
# Manually create in Cloudflare
www.example.com  CNAME  example.com
```

### Other Record Types

For MX, TXT, NS, SRV records:

**Use Cloudflare Dashboard directly.**

Auberge CLI focuses on A records for application subdomains.

## TTL (Time to Live)

### Default TTL

All records created with 300 seconds (5 minutes) TTL:

```bash
auberge dns set --subdomain cal --ip 10.0.0.1
# Creates record with TTL=300
```

### Custom TTL

Not currently supported via CLI.

**Workaround:** Update in Cloudflare Dashboard after creation.

**When to use lower TTL:**

- Before migration (easier to rollback)
- Testing new setup

**When to use higher TTL:**

- Stable production (reduces DNS queries)
- Cost optimization (fewer Cloudflare API calls)

## Proxied vs DNS-Only

### Current Behavior

Records created via CLI are **DNS-only** (not proxied through Cloudflare):

```bash
auberge dns set --subdomain cal --ip 194.164.53.11
# Creates DNS-only A record
```

**DNS-only** means:

- Direct connection to your VPS
- No Cloudflare caching
- No Cloudflare WAF
- VPS IP exposed in DNS

### Enabling Cloudflare Proxy

**Must be done manually** in Cloudflare Dashboard:

1. Navigate to DNS → Records
2. Click on the record
3. Toggle "Proxy status" to "Proxied" (orange cloud)

**Proxied** mode:

- Traffic routed through Cloudflare
- Cloudflare caching enabled
- DDoS protection
- VPS IP hidden

**Note:** DNS-01 ACME challenges work with both proxied and DNS-only records.

## DNS Propagation

### Immediate Effect

Changes via Cloudflare API are **immediate** for Cloudflare's DNS servers.

### Client Propagation

Clients may cache old values based on TTL:

```bash
# Set new IP
auberge dns set --subdomain cal --ip 10.0.0.1

# Some clients may still see old IP for up to TTL seconds
```

**Wait time:** Up to previous TTL value (default 300 seconds = 5 minutes).

### Verify Propagation

Check current DNS resolution:

```bash
# Query Cloudflare DNS directly
dig @1.1.1.1 cal.example.com +short

# Query your local resolver
dig cal.example.com +short

# Check multiple DNS servers
dig @8.8.8.8 cal.example.com +short  # Google DNS
dig @1.1.1.1 cal.example.com +short  # Cloudflare DNS
```

All should return the new IP within 5 minutes.

## Error Handling

### "Authentication error"

Cloudflare API token invalid or expired.

**Fix:** See [Cloudflare Setup](dns/cloudflare-setup.md)

### "Zone not found"

PRIMARY_DOMAIN environment variable incorrect.

**Fix:**

```bash
mise env | grep PRIMARY_DOMAIN
mise set --age-encrypt --prompt PRIMARY_DOMAIN
```

### "Invalid IP address"

IP format incorrect.

**Fix:** Use valid IPv4 format:

```bash
# Good
auberge dns set --subdomain cal --ip 194.164.53.11

# Bad
auberge dns set --subdomain cal --ip 194.164.53.11:22  # No port
auberge dns set --subdomain cal --ip example.com       # No hostname
```

### "Rate limit exceeded"

Too many API requests in short time.

**Fix:** Wait 60 seconds and retry. Cloudflare's free tier has generous limits.

## Best Practices

### Batch Operations Over Individual Sets

For multiple records:

✗ **Slow:**

```bash
auberge dns set --subdomain cal --ip 10.0.0.1
auberge dns set --subdomain rss --ip 10.0.0.1
auberge dns set --subdomain music --ip 10.0.0.1
```

✓ **Fast:**

```bash
auberge dns set-all --ip 10.0.0.1
```

See [Batch Operations](dns/batch-operations.md).

### Verify Before Deployment

Check DNS before running Ansible:

```bash
# Check current DNS status
auberge dns status

# Deploy if all records exist
auberge ansible run --host auberge
```

### Use Status for Health Checks

Automated monitoring:

```bash
# Check if all configured subdomains have A records
auberge dns status | grep "All configured subdomains"
# Exit code 0 if healthy
```

### Document Custom Subdomains

If you override defaults:

```toml
# mise.toml
# Custom subdomains (document why)
RADICALE_SUBDOMAIN = "cal"      # Shorter URL
NAVIDROME_SUBDOMAIN = "tunes"   # Branding
```

## Scripting with DNS Commands

### Shell Script Example

```bash
#!/bin/bash
set -euo pipefail

# Update DNS for new VPS
NEW_IP="10.0.0.1"
SUBDOMAINS=("cal" "rss" "music" "books" "files" "url")

for subdomain in "${SUBDOMAINS[@]}"; do
    echo "Updating $subdomain..."
    auberge dns set --subdomain "$subdomain" --ip "$NEW_IP"
    sleep 1  # Rate limiting
done

echo "DNS migration complete"
```

### Verify Script

```bash
#!/bin/bash

# Verify all subdomains resolve to expected IP
EXPECTED_IP="194.164.53.11"
DOMAIN="example.com"
SUBDOMAINS=("cal" "rss" "music")

for subdomain in "${SUBDOMAINS[@]}"; do
    ACTUAL_IP=$(dig +short "$subdomain.$DOMAIN" @1.1.1.1)
    if [ "$ACTUAL_IP" = "$EXPECTED_IP" ]; then
        echo "✓ $subdomain.$DOMAIN -> $ACTUAL_IP"
    else
        echo "✗ $subdomain.$DOMAIN -> $ACTUAL_IP (expected $EXPECTED_IP)"
        exit 1
    fi
done

echo "All DNS records verified"
```

## Related Pages

- [Cloudflare Setup](dns/cloudflare-setup.md) - Initial configuration
- [Migration](dns/migration.md) - Bulk IP migration
- [Batch Operations](dns/batch-operations.md) - Create multiple records
- [CI/CD](deployment/ci-cd.md) - Automated DNS updates
