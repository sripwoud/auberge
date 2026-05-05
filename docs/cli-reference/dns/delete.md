# auberge dns delete

Delete an A record for a subdomain

## Synopsis

```bash
auberge dns delete [-s <SUBDOMAIN>] [--dry-run] [--production] [--yes]
```

## Alias

`auberge d d`

## Description

Removes the Cloudflare A record for the given subdomain.

The command is **idempotent**: if no A record exists for the subdomain it
reports success with a clear "already gone" diagnostic instead of failing.

Confirmation is required by default. Production deletions escalate the
confirmation: the user must retype the subdomain name to proceed (sandbox
deletions use a simpler `[y/N]` prompt). Pass `--yes` to skip both prompts
in scripts or CI. When stdin is not a TTY (e.g. piped, CI), the command
exits without deleting unless `--yes` is set.

Only A records are considered. CNAME / AAAA / TXT records sharing the same
name are ignored — running this command against such a name reports
"nothing to delete" without touching them.

## Options

| Option           | Description                           | Required |
| ---------------- | ------------------------------------- | -------- |
| -s, --subdomain  | Subdomain name (omit to be prompted)  | No       |
| -n, --dry-run    | Preview without deleting              | No       |
| -y, --yes        | Skip confirmation prompt              | No       |
| -P, --production | Use production API (default: sandbox) | No       |

## Examples

```bash
# Pick a subdomain interactively, confirm, then delete (sandbox)
auberge dns delete

# Explicit subdomain, interactive confirmation
auberge dns delete -s freshrss

# Preview the action without deleting
auberge dns delete -s freshrss --dry-run

# Skip confirmation (useful in scripts)
auberge dns delete -s freshrss --yes

# Production delete in CI (no prompts)
auberge dns delete -s calibre --production --yes
```

## Output Examples

### Sandbox delete

```
CLOUDFLARE DNS
Delete A record for freshrss.example.com? yes
✓ A record deleted: freshrss.example.com
```

### Production delete (typed confirmation)

```
CLOUDFLARE DNS
Type 'freshrss' to confirm production deletion: freshrss
✓ A record deleted: freshrss.example.com
```

### Dry run

```
CLOUDFLARE DNS
→ [DRY RUN] Would delete A record: freshrss.example.com
```

### Already-absent record

```
CLOUDFLARE DNS
Delete A record for freshrss.example.com? yes
→ No A record found for freshrss.example.com — nothing to delete
```

## Behaviour

- **Record exists**: Deletes the A record via the Cloudflare API.
- **Record absent**: Returns success with an informational message.
- **Non-A records (CNAME/AAAA/TXT) for the same name**: Ignored; reported as absent.
- **Sandbox confirmation**: Interactive `[y/N]` prompt unless `--yes` is passed.
- **Production confirmation**: User must retype the subdomain name unless `--yes` is passed.
- **Non-TTY without `--yes`**: Exits without deleting (CI-safe; no hang on stdin).
- **Dry run**: Resolves the subdomain, prints what would be deleted, exits without contacting the Cloudflare delete endpoint.

## Use Cases

**Decommission a retired app**:

```bash
auberge dns delete -s oldapp --production --yes
```

**Cut over to tailnet-only DNS** (after publishing via Tailscale):

```bash
auberge dns delete -s myapp --production --yes
```

**Verify before acting** (preview the FQDN that would be deleted):

```bash
auberge dns delete -s myapp --production --dry-run
```

## Related Commands

- [auberge dns set](set.md) - Create or update an A record
- [auberge dns list](list.md) - List all records
- [auberge dns status](status.md) - Show DNS health
