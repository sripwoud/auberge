# auberge dns delete

Delete an A record for a subdomain

## Synopsis

```bash
auberge dns delete <SUBDOMAIN>
```

## Alias

`auberge d d`

## Description

Removes the Cloudflare A record for the given subdomain.

The command is **idempotent**: if no A record exists for the subdomain it
reports success with a clear "already gone" diagnostic instead of failing.

An interactive confirmation prompt is shown by default because Cloudflare
deletions are destructive. Pass `--yes` to skip it in scripts or CI.

## Options

| Option           | Description                           | Required |
| ---------------- | ------------------------------------- | -------- |
| SUBDOMAIN        | Subdomain name (positional)           | Yes      |
| -y, --yes        | Skip confirmation prompt              | No       |
| -P, --production | Use production API (default: sandbox) | No       |

## Examples

```bash
# Delete a record (interactive confirmation)
auberge dns delete freshrss

# Skip confirmation (useful in scripts)
auberge dns delete freshrss --yes

# Production API
auberge dns delete calibre --production --yes
```

## Output Example

```
CLOUDFLARE DNS
Deleting A record: freshrss.example.com
Delete A record for freshrss.example.com? [y/N]: y
✓ A record deleted successfully
```

### Already-absent record

```
CLOUDFLARE DNS
Deleting A record: freshrss.example.com
ℹ No A record found for freshrss.example.com — nothing to delete
```

## Behaviour

- **Record exists**: Deletes the A record via the Cloudflare API.
- **Record absent**: Returns success with an informational message.
- **Confirmation**: Interactive `[y/N]` prompt unless `--yes` is passed.

## Use Cases

**Decommission a retired app**:

```bash
auberge dns delete oldapp --production --yes
```

**Cut over to tailnet-only DNS** (after publishing via Tailscale):

```bash
auberge dns delete myapp --production --yes
```

## Related Commands

- [auberge dns set](set.md) - Create or update an A record
- [auberge dns list](list.md) - List all records
- [auberge dns status](status.md) - Show DNS health
