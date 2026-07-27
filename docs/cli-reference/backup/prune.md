# auberge backup prune

Prune old snapshots from the offsite restic repository

## Usage

```bash
auberge backup prune [OPTIONS]
```

## Options

- `-n, --dry-run` - Show what would be pruned without removing

## Retention Policy

- 7 daily snapshots
- 4 weekly snapshots
- 12 monthly snapshots

Retention applies per host: each snapshot is tagged with its host name at push, and `forget` groups by tag (`--group-by tags`). Hosts sharing one repository never evict each other's snapshots.

## Prerequisites

Same as [backup push](cli-reference/backup/push.md) — requires `restic_repository` and `restic_password` config values.

## Examples

```bash
# Preview what would be pruned
auberge backup prune --dry-run

# Apply retention policy and remove old snapshots
auberge backup prune
```
