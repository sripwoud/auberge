# auberge backup sync

Create backup, push to restic, prune, and clean up local staging

## Usage

```bash
auberge backup sync [OPTIONS]
# Alias: auberge b s
```

## Options

- `-H, --host` - Target host (interactive if omitted)
- `-a, --apps` - Apps to backup (comma-separated, default: all)
- `-k, --ssh-key` - SSH private key (default: ~/.ssh/identities/{user}\_{host})
- `--include-music` - Include music files in Navidrome backup
- `-n, --dry-run` - Preview the full pipeline without making changes
- `-v, --verbose` - Show detailed progress

## Pipeline

1. **Create** — rsync app data from VPS to local timestamped directory
2. **Push** — push the local backup to the offsite restic repository
3. **Prune** — apply retention policy (7 daily, 4 weekly, 12 monthly)
4. **Cleanup** — delete the local timestamped staging directory

The local staging copy is ephemeral — restic handles long-term retention with content-addressable, deduplicated storage.

## Prerequisites

Same as [backup push](push.md) — requires `restic_repository` and `restic_password` config values, plus `restic` and `rclone` installed.

## Examples

```bash
# Full sync for a specific host
auberge backup sync --host myserver

# Sync specific apps only
auberge backup sync --host myserver --apps baikal,freshrss

# Dry run (only runs create in dry-run mode, shows what would happen next)
auberge backup sync --host myserver --dry-run

# Include music in Navidrome backup
auberge backup sync --host myserver --include-music
```
