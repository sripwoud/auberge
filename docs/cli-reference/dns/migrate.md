# auberge dns migrate

Migrate all A records to new IP address

## Synopsis

```bash
auberge dns migrate --ip <IP> [OPTIONS]
```

## Alias

`auberge d m`

## Description

Updates all existing A records to point to a new IP address. Useful for server migrations.

Shows a table of old vs new IPs before making changes (dry run by default is recommended).

## Options

| Option           | Description                           | Default |
| ---------------- | ------------------------------------- | ------- |
| -i, --ip IP      | New IP address (required)             | None    |
| -n, --dry-run    | Preview without updating              | false   |
| -P, --production | Use production API (default: sandbox) | false   |

## Examples

```bash
# Preview migration
auberge dns migrate --ip 10.0.0.5 --dry-run

# Execute migration
auberge dns migrate --ip 10.0.0.5

# Production API
auberge dns migrate --ip 10.0.0.5 --production
```

## Dry Run Output

```
CLOUDFLARE DNS
[DRY RUN] DNS Migration Preview
--------------------------------------------------
SUBDOMAIN      CURRENT           NEW
--------------------------------------------------
blocky         192.168.1.10  ->  10.0.0.5
freshrss       192.168.1.10  ->  10.0.0.5
radicale       192.168.1.10  ->  10.0.0.5
calibre        192.168.1.10  ->  10.0.0.5
navidrome      192.168.1.10  ->  10.0.0.5

Would update 5 A record(s).
```

## Execution Output

```
CLOUDFLARE DNS
DNS Migration
--------------------------------------------------
SUBDOMAIN      CURRENT           NEW
--------------------------------------------------
blocky         192.168.1.10  ->  10.0.0.5
freshrss       192.168.1.10  ->  10.0.0.5
radicale       192.168.1.10  ->  10.0.0.5

Updated 3 A record(s).
```

## Migration Process

1. **Query all A records** from Cloudflare
2. **Filter records** to migrate (only A records)
3. **Show preview** of changes
4. **Update each record** to new IP
5. **Report results**

## Use Cases

**VPS migration**:

```bash
# 1. Preview changes
auberge dns migrate --ip 10.0.0.5 --dry-run

# 2. Verify preview is correct
# 3. Execute migration
auberge dns migrate --ip 10.0.0.5

# 4. Verify records
auberge dns list
```

**Disaster recovery**:

```bash
# Quickly point all records to new server
auberge dns migrate --ip 203.0.113.5
```

## Workflow for Server Migration

1. **Prepare new server**:
   ```bash
   # Bootstrap new server
   auberge ansible bootstrap newserver --ip 10.0.0.5

   # Deploy applications
   auberge ansible run --host newserver
   ```

2. **Test before DNS migration**:
   ```bash
   # Add to /etc/hosts for testing
   echo "10.0.0.5 freshrss.example.com" | sudo tee -a /etc/hosts

   # Test services
   curl https://freshrss.example.com
   ```

3. **Migrate DNS** (dry run first):
   ```bash
   auberge dns migrate --ip 10.0.0.5 --dry-run
   auberge dns migrate --ip 10.0.0.5
   ```

4. **Verify migration**:
   ```bash
   auberge dns list
   dig freshrss.example.com
   ```

## Safety Features

**Dry run recommended**: Always preview with --dry-run first
**Only A records**: Doesn't touch CNAME, MX, TXT, etc.
**Atomic updates**: Each record updated independently

## Record Types Affected

**Affected**:

- A records (IPv4)

**Not affected**:

- AAAA records (IPv6) - use set-all or set individually
- CNAME records
- MX records
- TXT records
- NS records

## Troubleshooting

**No records updated**:

- Check you have A records: `auberge dns list`
- Verify IP format is valid

**Partial update**:

- Some records updated, some failed
- Check Cloudflare API status
- Manually fix failed records with `auberge dns set`

## Related Commands

- [auberge dns list](list.md) - Verify migration
- [auberge dns set](set.md) - Fix individual records
- [auberge dns set-all](set-all.md) - Bulk create records

## See Also

- [DNS Management](../../dns/README.md)
- [Migration Guide](../../backup-restore/migration.md)
