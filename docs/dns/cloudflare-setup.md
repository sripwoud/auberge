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

### Via mise (Recommended)

```bash
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
# Paste your token when prompted
```

This encrypts the token with age and stores it in `mise.toml`.

### Via Environment Variable (Testing)

```bash
export CLOUDFLARE_DNS_API_TOKEN="your-token-here"
```

**Warning:** Not persistent across sessions. Use mise for production.

## Step 4: Set Primary Domain

```bash
mise set --age-encrypt --prompt PRIMARY_DOMAIN
# Enter your domain (e.g., example.com)
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

## Subdomain Configuration

### Default Subdomains

Auberge uses environment variables for subdomain names:

```toml
# mise.toml [env] section (not encrypted - public config)
BLOCKY_SUBDOMAIN = "dns"
CALIBRE_SUBDOMAIN = "lire"
FRESHRSS_SUBDOMAIN = "rss"
NAVIDROME_SUBDOMAIN = "musique"
RADICALE_SUBDOMAIN = "calendrier"
WEBDAV_SUBDOMAIN = "webdav"
YOURLS_SUBDOMAIN = "url"
```

**Result:** Applications accessible at:

- `dns.example.com` (Blocky)
- `lire.example.com` (Calibre)
- `rss.example.com` (FreshRSS)
- `musique.example.com` (Navidrome)
- `calendrier.example.com` (Radicale)
- `webdav.example.com` (WebDAV)
- `url.example.com` (YOURLS)

### Custom Subdomains

Override defaults in `mise.toml`:

```toml
[env]
RADICALE_SUBDOMAIN = "cal"        # calendrier → cal
NAVIDROME_SUBDOMAIN = "music"     # musique → music
FRESHRSS_SUBDOMAIN = "feeds"      # rss → feeds
```

**Result:**

- `cal.example.com` (Radicale)
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
# Update mise
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
# Paste new token

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
2. Update mise:
   ```bash
   mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
   ```

### "Zone not found"

Domain not configured in Cloudflare or wrong domain in mise.

**Fix:**

```bash
# Verify domain in mise
mise env | grep PRIMARY_DOMAIN

# Update if wrong
mise set --age-encrypt --prompt PRIMARY_DOMAIN
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

## Configuration File (Optional)

For CLI DNS commands only (not needed for Ansible):

```bash
# Copy example config
cp config.example.toml config.toml

# Edit config
vim config.toml
```

```toml
[dns]
domain = "example.com"
default_ttl = 300

[cloudflare]
# Optional: speeds up API calls by skipping zone discovery
# zone_id = "your-zone-id-here"
```

**Note:** This is optional - DNS commands work without it by using environment variables.

## Related Pages

- [Managing Records](dns/managing-records.md) - DNS record operations
- [Migration](dns/migration.md) - Migrate DNS to new IP
- [Batch Operations](dns/batch-operations.md) - Bulk record creation
- [Secrets Management](configuration/secrets.md) - Encrypting sensitive data
