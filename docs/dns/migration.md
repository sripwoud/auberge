# DNS Migration

Migrate all A records to a new IP address in one command.

## Overview

The `dns migrate` command updates all existing A records to point to a new IP address. Useful for:

- VPS provider migration
- Disaster recovery
- IP address changes

## Command

```bash
auberge dns migrate --ip NEW_IP
```

**Example:**

```bash
auberge dns migrate --ip 10.0.0.1
```

## Dry Run

Preview changes before applying:

```bash
auberge dns migrate --ip 10.0.0.1 --dry-run
```

**Example output:**

```
[DRY RUN] DNS Migration Preview
--------------------------------------------------
SUBDOMAIN      CURRENT              NEW
--------------------------------------------------
cal            194.164.53.11    ->  10.0.0.1
rss            194.164.53.11    ->  10.0.0.1
music          194.164.53.11    ->  10.0.0.1
books          194.164.53.11    ->  10.0.0.1
files          194.164.53.11    ->  10.0.0.1
url            194.164.53.11    ->  10.0.0.1

Would update 6 A record(s).
```

## How It Works

1. **Fetch all existing A records** from Cloudflare
2. **Filter to your domain** (excludes apex and www)
3. **Update each record** to new IP
4. **Display results**

**Important:** Only updates **existing** A records. Does not create new ones.

## Use Cases

### VPS Provider Migration

Moving from one provider to another:

```bash
# Old VPS: 194.164.53.11 (IONOS)
# New VPS: 10.0.0.1 (Hetzner)

# 1. Deploy to new VPS
auberge ansible bootstrap new-vps --ip 10.0.0.1
auberge ansible run --host new-vps --skip-tags bootstrap

# 2. Restore data to new VPS
auberge backup restore latest --from-host old-vps --host new-vps

# 3. Dry run migration
auberge dns migrate --ip 10.0.0.1 --dry-run

# 4. Migrate DNS
auberge dns migrate --ip 10.0.0.1

# 5. Verify
dig +short cal.example.com  # Should return 10.0.0.1
```

### Disaster Recovery

Old VPS is down, migrate to backup:

```bash
# Provision new VPS at 10.0.0.1
# Restore from backup
# Update DNS

auberge dns migrate --ip 10.0.0.1
```

### IP Address Change

Provider changed your IP:

```bash
# Old IP: 194.164.53.11
# New IP: 194.164.53.99

auberge dns migrate --ip 194.164.53.99
```

## Behavior

### What Gets Migrated

**Included:**

- All A records for subdomains
- Records pointing to any IP

**Excluded:**

- Apex domain (example.com) - updated separately if needed
- www subdomain - often a CNAME, not an A record
- AAAA records (IPv6)
- CNAME, MX, TXT, NS records

**Example:**

**Before migration:**

```
cal.example.com       A      194.164.53.11
rss.example.com       A      194.164.53.11
staging.example.com   A      10.0.0.50      # Different IP
www.example.com       CNAME  example.com
```

**After `auberge dns migrate --ip 10.0.0.1`:**

```
cal.example.com       A      10.0.0.1       # Updated
rss.example.com       A      10.0.0.1       # Updated
staging.example.com   A      10.0.0.1       # Updated (even though different)
www.example.com       CNAME  example.com    # Not changed (CNAME)
```

### Selective Migration

If you don't want to migrate all records:

**Use case:** Keep staging on different IP

**Solution:** Use `dns set` for individual records:

```bash
# Migrate production only
auberge dns set --subdomain cal --ip 10.0.0.1
auberge dns set --subdomain rss --ip 10.0.0.1
# Leave staging.example.com at 10.0.0.50
```

Or use `dns set-all` with filtering:

```bash
# Create specific records at new IP
auberge dns set-all --ip 10.0.0.1 --subdomains cal,rss,music
```

## Verification

After migration, verify DNS propagation:

```bash
# Check each subdomain
dig +short cal.example.com @1.1.1.1
dig +short rss.example.com @1.1.1.1
dig +short music.example.com @1.1.1.1

# All should return new IP
```

Or use status command:

```bash
auberge dns status
```

**Output:**

```
Active A records: 6
  cal.example.com -> 10.0.0.1
  rss.example.com -> 10.0.0.1
  ...
```

## Migration Workflow

Complete VPS migration workflow:

```bash
# 1. Backup old VPS
auberge backup create --host old-vps

# 2. Bootstrap new VPS
export NEW_VPS_IP="10.0.0.1"
mise set --age-encrypt NEW_VPS_HOST
auberge ansible bootstrap new-vps --ip $NEW_VPS_IP

# 3. Deploy stack to new VPS
auberge ansible run --host new-vps --skip-tags bootstrap

# 4. Restore data
auberge backup restore latest --from-host old-vps --host new-vps

# 5. Preview DNS migration
auberge dns migrate --ip $NEW_VPS_IP --dry-run

# 6. Migrate DNS
auberge dns migrate --ip $NEW_VPS_IP

# 7. Verify services
curl -I https://cal.example.com
curl -I https://rss.example.com

# 8. Decommission old VPS (after confirming everything works)
```

## Rollback

If migration goes wrong, revert to old IP:

```bash
# Rollback DNS
auberge dns migrate --ip 194.164.53.11  # Old IP

# Verify
auberge dns status
```

**DNS propagation:** Changes take up to 5 minutes (default TTL).

## Best Practices

### Always Dry Run First

```bash
# Check what would change
auberge dns migrate --ip 10.0.0.1 --dry-run

# Review output carefully
# Then execute
auberge dns migrate --ip 10.0.0.1
```

### Lower TTL Before Migration

Reduce TTL before migration for faster rollback:

1. Manually set TTL to 60 seconds in Cloudflare Dashboard
2. Wait for old TTL to expire (5 minutes)
3. Run migration
4. Verify everything works
5. Restore TTL to 300 seconds

### Monitor After Migration

Watch service logs for errors:

```bash
ssh ansible@new-vps "journalctl -f | grep -i error"
```

### Keep Old VPS Running

Don't decommission old VPS immediately:

- Keep for 24-48 hours
- Verify all services on new VPS
- Then delete old VPS

## Comparison with set-all

| Feature             | `dns migrate`                 | `dns set-all`                            |
| ------------------- | ----------------------------- | ---------------------------------------- |
| **Purpose**         | Update existing records       | Create/update all configured records     |
| **Source**          | Cloudflare (existing records) | Environment vars (configured subdomains) |
| **Missing records** | Ignores                       | Creates                                  |
| **Extra records**   | Migrates                      | Ignores                                  |
| **Use case**        | VPS migration                 | Initial DNS setup                        |

**Example difference:**

**Cloudflare has:**

- cal.example.com → 194.164.53.11
- old-app.example.com → 194.164.53.11

**Environment vars have:**

- RADICALE_SUBDOMAIN=cal
- FRESHRSS_SUBDOMAIN=rss

**After `dns migrate --ip 10.0.0.1`:**

- cal.example.com → 10.0.0.1 (migrated)
- old-app.example.com → 10.0.0.1 (migrated)
- rss.example.com → (not created, doesn't exist)

**After `dns set-all --ip 10.0.0.1`:**

- cal.example.com → 10.0.0.1 (created/updated)
- rss.example.com → 10.0.0.1 (created)
- old-app.example.com → 194.164.53.11 (not touched)

**Choose:**

- `migrate` for moving all records to new IP
- `set-all` for creating configured apps on new IP

## Troubleshooting

### "No A records found"

No A records exist in Cloudflare for your domain.

**Fix:** Create records first:

```bash
auberge dns set-all --ip 10.0.0.1
```

### "Some records failed to update"

Partial migration due to API errors.

**Fix:** Re-run command (idempotent):

```bash
auberge dns migrate --ip 10.0.0.1
```

### Old IP still resolving

DNS cache or propagation delay.

**Fix:** Wait 5 minutes, flush DNS cache:

```bash
# Linux
sudo systemd-resolve --flush-caches

# macOS
sudo dscacheutil -flushcache

# Test with specific DNS server
dig @1.1.1.1 cal.example.com +short
```

## Alias

Short form:

```bash
auberge dns m --ip 10.0.0.1
auberge dns m --ip 10.0.0.1 -n  # dry-run
```

## Related Pages

- [Batch Operations](dns/batch-operations.md) - Creating multiple records
- [Managing Records](dns/managing-records.md) - Individual record management
- [Cross-Host Migration](backup-restore/cross-host-migration.md) - VPS data migration
- [Bootstrap](deployment/bootstrap.md) - New VPS setup
