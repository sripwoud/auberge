# Batch DNS Operations

Create multiple DNS A records in one command using `dns set-all`.

## Overview

`dns set-all` creates or updates A records for all configured application subdomains, pointing them to a specified IP address.

**Use cases:**

- Initial DNS setup for new deployment
- Bulk record creation
- Synchronizing DNS with configuration

## Basic Usage

### With Host

```bash
auberge dns set-all --host auberge
```

Automatically uses the IP address from inventory for the specified host.

### With Explicit IP

```bash
auberge dns set-all --ip 194.164.53.11
```

Uses the provided IP address.

## How It Works

1. **Discover configured subdomains** from environment variables
   - Reads `*_SUBDOMAIN` environment vars
   - Example: `BAIKAL_SUBDOMAIN=cal`

2. **Determine target IP**
   - From `--host` (lookups IP in inventory)
   - Or from `--ip` flag

3. **Display preview** of records to create

4. **Confirm** (unless `--yes` flag used)

5. **Create/update A records** in Cloudflare

## Example

```bash
$ auberge dns set-all --host auberge

CLOUDFLARE DNS

Creating the following A records:
  • dns.example.com → 194.164.53.11
  • lire.example.com → 194.164.53.11
  • rss.example.com → 194.164.53.11
  • musique.example.com → 194.164.53.11
  • calendrier.example.com → 194.164.53.11
  • webdav.example.com → 194.164.53.11
  • url.example.com → 194.164.53.11

Proceed? [y/N]: y

✓ Created dns.example.com
✓ Created lire.example.com
✓ Created rss.example.com
✓ Created musique.example.com
✓ Created calendrier.example.com
✓ Created webdav.example.com
✓ Created url.example.com

✓ Successfully created 7/7 A records pointing to 194.164.53.11
```

## Options

### Dry Run

Preview without making changes:

```bash
auberge dns set-all --host auberge --dry-run
```

### Skip Confirmation

Non-interactive execution:

```bash
auberge dns set-all --host auberge --yes
```

### Specific Subdomains

Only create selected records:

```bash
auberge dns set-all --host auberge --subdomains cal,rss,music
```

### Skip Subdomains

Exclude specific records:

```bash
auberge dns set-all --host auberge --skip dns,url
```

### Continue on Error

Don't stop if one record fails:

```bash
auberge dns set-all --host auberge --continue-on-error
```

## Use Cases

### Initial DNS Setup

After deploying to fresh VPS:

```bash
# Deploy infrastructure
auberge ansible run --host auberge --skip-tags bootstrap

# Create all DNS records
auberge dns set-all --host auberge
```

### New Application

Added new app to configuration:

```bash
# Add subdomain to mise.toml
echo 'NEWAPP_SUBDOMAIN = "newapp"' >> mise.toml

# Create DNS record
auberge dns set-all --host auberge --subdomains newapp
```

### Selective Update

Update specific apps after migration:

```bash
# Only update media apps
auberge dns set-all --ip 10.0.0.1 --subdomains music,books
```

### CI/CD Integration

Automated DNS updates:

```bash
auberge dns set-all --host production --yes --continue-on-error
```

## Environment Variable Discovery

`set-all` reads subdomain names from environment variables:

**Format:** `{APP}_SUBDOMAIN`

**Default values (from mise.toml):**

```toml
BLOCKY_SUBDOMAIN = "dns"
CALIBRE_SUBDOMAIN = "lire"
FRESHRSS_SUBDOMAIN = "rss"
NAVIDROME_SUBDOMAIN = "musique"
BAIKAL_SUBDOMAIN = "calendrier"
WEBDAV_SUBDOMAIN = "webdav"
YOURLS_SUBDOMAIN = "url"
```

**Result:** Creates A records for:

- dns.example.com
- lire.example.com
- rss.example.com
- musique.example.com
- calendrier.example.com
- webdav.example.com
- url.example.com

### Custom Subdomains

Override defaults:

```toml
# mise.toml
BAIKAL_SUBDOMAIN = "cal"      # calendrier → cal
NAVIDROME_SUBDOMAIN = "music"   # musique → music
```

`set-all` will use `cal` and `music` instead.

## Host vs IP

### Using --host

```bash
auberge dns set-all --host auberge
```

**Behavior:**

- Looks up IP for `auberge` in inventory
- Uses `ansible_host` value
- Requires host configured in inventory

**Example inventory:**

```yaml
hosts:
  auberge:
    ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
```

### Using --ip

```bash
auberge dns set-all --ip 194.164.53.11
```

**Behavior:**

- Uses provided IP directly
- Doesn't require inventory
- Useful for testing or one-off operations

### Conflicts

Can't use both:

```bash
# Error: conflicts
auberge dns set-all --host auberge --ip 10.0.0.1
```

Choose one or the other.

## Filtering Options

### --subdomains (Include Only)

Create only specified subdomains:

```bash
auberge dns set-all --host auberge --subdomains cal,rss
```

**Only creates:**

- cal.example.com
- rss.example.com

**Skips:** All others (dns, music, books, etc.)

### --skip (Exclude)

Create all except specified:

```bash
auberge dns set-all --host auberge --skip dns,url
```

**Creates:** lire, rss, musique, calendrier, webdav

**Skips:** dns, url

### Combine Both

Include specific set, then exclude from that:

```bash
auberge dns set-all --host auberge --subdomains cal,rss,music --skip music
```

**Result:** Only creates cal and rss (music excluded)

## Error Handling

### Default: Fail Fast

Stops on first error:

```bash
$ auberge dns set-all --host auberge

✓ Created dns.example.com
✓ Created cal.example.com
✗ Failed rss.example.com: Rate limit exceeded
# Stops here, doesn't create remaining records
```

### With --continue-on-error

Attempts all records despite failures:

```bash
$ auberge dns set-all --host auberge --continue-on-error

✓ Created dns.example.com
✓ Created cal.example.com
✗ Failed rss.example.com: Rate limit exceeded
✓ Created music.example.com
✓ Created books.example.com
# Continues through all records

✓ Successfully created 4/5 A records pointing to 194.164.53.11
Failed to create 1 records
```

**Use case:** CI/CD where partial success is acceptable.

## Output Formats

### Human (Default)

Readable output with colors and symbols:

```bash
auberge dns set-all --host auberge
```

### JSON

Machine-readable output:

```bash
auberge dns set-all --host auberge --output json
```

**Future feature** - not yet implemented.

### TSV

Tab-separated values:

```bash
auberge dns set-all --host auberge --output tsv
```

**Future feature** - not yet implemented.

## Best Practices

### Always Dry Run First

```bash
# Preview
auberge dns set-all --host auberge --dry-run

# If looks good, execute
auberge dns set-all --host auberge
```

### Use in Deployment Scripts

```bash
#!/bin/bash
set -euo pipefail

# Deploy VPS
auberge ansible run --host production --skip-tags bootstrap

# Set up DNS
auberge dns set-all --host production --yes

# Verify
auberge dns status
```

### Selective Updates

For large configs, update only what changed:

```bash
# Only new apps
auberge dns set-all --host auberge --subdomains newapp1,newapp2
```

### Error Handling in Scripts

```bash
if ! auberge dns set-all --host auberge --yes; then
    echo "DNS setup failed, rolling back deployment"
    # Rollback logic here
    exit 1
fi
```

## Comparison with Other Commands

| Command   | Purpose                            | Creates New | Updates Existing |
| --------- | ---------------------------------- | ----------- | ---------------- |
| `set-all` | Batch create configured subdomains | Yes         | Yes              |
| `migrate` | Update all existing records        | No          | Yes              |
| `set`     | Single subdomain                   | Yes         | Yes              |

**When to use `set-all`:**

- Initial DNS setup
- Adding multiple apps
- Synchronizing DNS with config

**When to use `migrate`:**

- VPS provider migration
- IP address change for all services

**When to use `set`:**

- Single record update
- Custom subdomain not in config

## Troubleshooting

### "No subdomain environment variables found"

No `*_SUBDOMAIN` env vars configured.

**Fix:**

```bash
# Check environment
mise env | grep SUBDOMAIN

# If empty, verify mise.toml has subdomain config
```

### "Host not found in inventory"

Specified host doesn't exist.

**Fix:**

```bash
# List available hosts
ansible-inventory -i ansible/inventory.yml --list

# Or use IP instead
auberge dns set-all --ip 194.164.53.11
```

### "Rate limit exceeded"

Too many API requests.

**Fix:** Wait 60 seconds, then retry with --continue-on-error:

```bash
auberge dns set-all --host auberge --continue-on-error
```

### Records already exist

Command is idempotent - safe to re-run:

```bash
# First run: creates records
auberge dns set-all --host auberge

# Second run: updates if IP changed, otherwise no-op
auberge dns set-all --host auberge
```

## Alias

Short form:

```bash
auberge dns sa --host auberge
auberge dns sa --ip 10.0.0.1 -n    # dry-run
auberge dns sa --host prod -y      # yes
```

## Related Pages

- [Managing Records](dns/managing-records.md) - Individual record operations
- [Migration](dns/migration.md) - Bulk IP migration
- [Cloudflare Setup](dns/cloudflare-setup.md) - Initial configuration
- [Environment Variables](configuration/environment-variables.md) - Subdomain configuration
