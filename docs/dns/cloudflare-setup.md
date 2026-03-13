# Cloudflare Setup

Auberge uses Cloudflare for DNS management and automatic HTTPS certificate provisioning via DNS-01 ACME challenges.

## Prerequisites

- Domain registered and using Cloudflare nameservers
- Cloudflare account (free tier works fine)
- API token with DNS edit permissions

## Step 1: Add Domain to Cloudflare

If your domain isn't already on Cloudflare:

1. Log into [Cloudflare Dashboard](https://dash.cloudflare.com)
2. Click "Add a Site"
3. Enter your domain name
4. Select a plan (Free works for most use cases)
5. Update nameservers at your domain registrar to Cloudflare's nameservers
6. Wait for DNS propagation (can take up to 24 hours)

## Step 2: Create API Token

### Navigate to API Tokens

1. Log into [Cloudflare Dashboard](https://dash.cloudflare.com)
2. Click on your profile icon (top right)
3. Select "My Profile"
4. Navigate to "API Tokens" tab
5. Click "Create Token"

### Use Template

Select the "Edit zone DNS" template:

- **Template:** Edit zone DNS
- **Permissions:**
  - Zone → DNS → Edit
  - Zone → Zone → Read
- **Zone Resources:**
  - Include → Specific zone → [your domain]

### Optional: IP Whitelisting

Add your VPS IP address for extra security:

- **Client IP Address Filtering:**
  - Include: [your VPS IP]

**Note:** This restricts the token to only work from your VPS. Omit if you want to use the token locally.

### Create and Copy Token

1. Click "Continue to summary"
2. Click "Create Token"
3. **Copy the token immediately** - it's only shown once

**Example token:**

```
gJ4kX-yF8nP2wQ5vR7tH9mL3bK6cN1dA4sZ8xE0fY2
```

## Step 3: Store Token Securely

Initialize config (if not already done) and add the token:

```bash
auberge config init
auberge config set cloudflare_dns_api_token your-token-here
```

## Step 4: Set Domain

```bash
auberge config set domain example.com
```

## Step 5: Verify Configuration

Test DNS connectivity:

```bash
auberge dns status
```

**Expected output:**

```
CLOUDFLARE DNS

DNS Status for example.com
----------------------------------------

Configured subdomains: dns, lire, rss, musique, calendrier, webdav, url

Active A records: 0

Missing subdomains: dns, lire, rss, musique, calendrier, webdav, url
```

If you see this, your API token is working correctly.

> **Note:** DNS records for app subdomains are created automatically when you deploy apps. You do not need to create them before deployment.

## Subdomain Configuration

### Default Subdomains

Auberge uses `config.toml` for subdomain names:

```toml
blocky_subdomain = "dns"
freshrss_subdomain = "rss"
navidrome_subdomain = "musique"
baikal_subdomain = "calendrier"
webdav_subdomain = "webdav"
yourls_subdomain = "url"
```

**Result:** Applications accessible at:

- `dns.example.com` (Blocky)
- `rss.example.com` (FreshRSS)
- `musique.example.com` (Navidrome)
- `calendrier.example.com` (Baikal)
- `webdav.example.com` (WebDAV)
- `url.example.com` (YOURLS)

### Custom Subdomains

Override defaults in `config.toml`:

```toml
baikal_subdomain = "cal"
navidrome_subdomain = "music"
freshrss_subdomain = "feeds"
```

**Result:**

- `cal.example.com` (Baikal)
- `music.example.com` (Navidrome)
- `feeds.example.com` (FreshRSS)

## Token Permissions Reference

### Required Permissions

**Zone → DNS → Edit:**

- Create A records
- Update existing records
- Delete records

**Zone → Zone → Read:**

- List zones
- Read zone details
- Required for DNS-01 ACME challenges

### Recommended Permissions

For full Auberge functionality:

| Permission | Level | Purpose            |
| ---------- | ----- | ------------------ |
| DNS        | Edit  | Manage DNS records |
| Zone       | Read  | Read zone info     |

### Avoid

Don't grant more permissions than necessary:

✗ Zone → Zone → Edit (not needed)
✗ Account-level permissions (too broad)
✗ Purge Cache (unrelated)

## Security Best Practices

### Use Dedicated Token

Create a token specifically for Auberge:

✓ **Good:** "Auberge DNS Management"
✗ **Bad:** Reusing your Global API Key

### Limit Token Scope

Restrict to specific zone:

✓ **Good:** Include → Specific zone → example.com
✗ **Bad:** Include → All zones

### Rotate Tokens Regularly

Change tokens periodically:

```bash
# Create new token in Cloudflare Dashboard
# Update config.toml
auberge config set cloudflare_dns_api_token your-new-token

# Verify it works
auberge dns status
```

### Revoke Compromised Tokens

If token is leaked:

1. Immediately revoke in Cloudflare Dashboard
2. Create new token
3. Update mise configuration
4. Rotate any other potentially affected secrets

## Troubleshooting

### "Authentication error"

Token is invalid or expired.

**Fix:**

1. Create new token in Cloudflare
2. Update config:
   ```bash
   auberge config set cloudflare_dns_api_token your-new-token
   ```

### "Zone not found"

Domain not configured in Cloudflare or wrong domain in `config.toml`.

**Fix:**

```bash
# Verify domain
auberge config get domain

# Update if wrong
auberge config set domain example.com
```

### "Insufficient permissions"

Token doesn't have required permissions.

**Fix:** Recreate token with correct permissions (see Required Permissions above).

### "Rate limit exceeded"

Too many API requests in short time.

**Fix:** Wait 1-2 minutes and retry. Cloudflare rate limits are generous.

## Alternative: Global API Key

**Not recommended** but possible:

1. Navigate to Cloudflare Dashboard → My Profile → API Tokens
2. View "Global API Key"
3. Store as environment variable

**Why not recommended:**

- Too broad permissions
- Higher security risk
- No IP restrictions
- Can't be scoped to single zone

Use API tokens instead.

## Configuration

Initialize and set your domain for CLI DNS commands:

```bash
auberge config init
auberge config set domain example.com
```

## Related Pages

- [Managing Records](dns/managing-records.md) - DNS record operations
- [Migration](dns/migration.md) - Migrate DNS to new IP
- [Batch Operations](dns/batch-operations.md) - Bulk record creation
- [Secrets Management](configuration/secrets.md) - Encrypting sensitive data
