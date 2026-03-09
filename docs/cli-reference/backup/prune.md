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

## Prerequisites

Same as [backup push](push.md) — requires `restic_repository` and `restic_password` config values.

## Examples

```bash
# Preview what would be pruned
auberge backup prune --dry-run

# Apply retention policy and remove old snapshots
auberge backup prune
```
