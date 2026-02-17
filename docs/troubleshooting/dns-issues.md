# DNS Issues

Troubleshooting Cloudflare DNS problems.

## Authentication Errors

### "Authentication error"

**Problem:** Invalid or expired Cloudflare API token.

**Solution:**

```bash
# Create new token in Cloudflare Dashboard
# Update mise
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN

# Test
auberge dns status
```

### "Insufficient permissions"

**Problem:** Token doesn't have required permissions.

**Solution:**

- Recreate token with DNS Edit + Zone Read permissions
- See [Cloudflare Setup](dns/cloudflare-setup.md)

## Zone Errors

### "Zone not found"

**Problem:** PRIMARY_DOMAIN incorrect or domain not in Cloudflare.

**Solution:**

```bash
# Check domain
mise env | grep PRIMARY_DOMAIN

# Update if wrong
mise set --age-encrypt --prompt PRIMARY_DOMAIN

# Verify domain in Cloudflare Dashboard
```

### "Multiple zones found"

**Problem:** Domain exists in multiple Cloudflare accounts.

**Solution:**

- Specify zone_id in config.toml
- Or ensure token is scoped to specific zone

## Record Errors

### "Record already exists"

**Problem:** Trying to create existing record.

**Solution:**

- DNS commands are idempotent - safe to retry
- Record will be updated, not duplicated

### "Invalid IP address"

**Problem:** IP format incorrect.

**Solution:**

```bash
# Use valid IPv4 format
auberge dns set --subdomain cal --ip 203.0.113.10

# Not:
# - Hostname
# - IPv6 (not supported)
# - IP with port
```

## Propagation Issues

### DNS not resolving

**Problem:** New record not resolving.

**Causes:**

1. **Just created** - Wait up to 5 minutes (TTL)
2. **DNS cache** - Flush local DNS cache
3. **Wrong record** - Verify record was created

**Solutions:**

```bash
# Check Cloudflare directly
dig @1.1.1.1 subdomain.example.com +short

# Flush cache
sudo systemd-resolve --flush-caches  # Linux
sudo dscacheutil -flushcache         # macOS

# Verify record exists
auberge dns list --subdomain cal
```

### Old IP still resolving

**Problem:** Updated record but old IP shows.

**Causes:**

- DNS cache
- TTL not expired
- Record not actually updated

**Solutions:**

```bash
# Wait for TTL to expire (5 minutes)
# Test with Cloudflare DNS directly
dig @1.1.1.1 subdomain.example.com +short

# Verify record was updated
auberge dns list --subdomain cal
```

## SSL/TLS Certificate Issues

### Certificate errors after DNS update

**Problem:** Let's Encrypt can't verify domain.

**Causes:**

- DNS not propagated yet
- Cloudflare proxy blocking verification
- Port 80/443 blocked

**Solutions:**

```bash
# Wait 5-10 minutes for DNS propagation

# Check Caddy logs
ssh ansible@vps "journalctl -u caddy -n 50"

# Restart Caddy to retry
ssh ansible@vps "sudo systemctl restart caddy"

# Verify DNS resolves
dig subdomain.example.com +short
```

### "DNS-01 challenge failed"

**Problem:** Caddy can't update DNS for certificate.

**Causes:**

- Cloudflare API token invalid
- Token missing DNS edit permissions

**Solutions:**

```bash
# Verify token is set
mise env | grep CLOUDFLARE_DNS_API_TOKEN

# Test token works
auberge dns status

# Recreate token if needed
# See [Cloudflare Setup](dns/cloudflare-setup.md)
```

## Rate Limiting

### "Rate limit exceeded"

**Problem:** Too many API requests.

**Causes:**

- Rapid repeated commands
- Script making many requests

**Solutions:**

```bash
# Wait 60 seconds
# Retry

# Use batch operations instead of loops
auberge dns set-all --host vps  # Not individual sets
```

## Migration Issues

### "Some records failed to migrate"

**Problem:** Partial migration failure.

**Solution:**

```bash
# Re-run (idempotent)
auberge dns migrate --ip 10.0.0.1

# Or use continue-on-error
auberge dns set-all --ip 10.0.0.1 --continue-on-error
```

### Unexpected records migrated

**Problem:** `dns migrate` updated records you didn't want changed.

**Solution:**

- Use `dns set-all` with filtering instead
- Or set individual records:
  ```bash
  auberge dns set --subdomain cal --ip 10.0.0.1
  auberge dns set --subdomain rss --ip 10.0.0.1
  ```

## Batch Operation Issues

### "No subdomain environment variables found"

**Problem:** No `*_SUBDOMAIN` vars configured.

**Solution:**

```bash
# Check environment
mise env | grep SUBDOMAIN

# Verify mise.toml has subdomain config
cat mise.toml | grep SUBDOMAIN
```

### Records created but apps unreachable

**Problem:** DNS works but apps not responding.

**Causes:**

- Apps not deployed
- Caddy not configured
- Firewall blocking ports

**Solutions:**

```bash
# Deploy apps
auberge ansible run --host vps --tags apps

# Check Caddy config
ssh ansible@vps "sudo systemctl status caddy"

# Check firewall
ssh ansible@vps "sudo ufw status"
```

## Debugging

### Verbose output

```bash
# DNS commands don't have verbose flag
# Check output carefully for errors
```

### Manual verification

```bash
# List all records
auberge dns list

# Check specific record
dig subdomain.example.com +short

# Query different DNS servers
dig @1.1.1.1 subdomain.example.com  # Cloudflare
dig @8.8.8.8 subdomain.example.com  # Google
```

### Cloudflare Dashboard

Verify records in Cloudflare web UI:

1. Log into [Cloudflare Dashboard](https://dash.cloudflare.com)
2. Select your domain
3. Navigate to DNS → Records
4. Check A records

## Related Pages

- [Cloudflare Setup](dns/cloudflare-setup.md)
- [Managing Records](dns/managing-records.md)
- [Migration](dns/migration.md)
- [Batch Operations](dns/batch-operations.md)
