# auberge backup push

Push local backups to an offsite restic repository

## Usage

```bash
auberge backup push [OPTIONS]
# Alias: auberge b p
```

## Options

- `-H, --host` - Filter backups by host (interactive if multiple hosts exist)
- `-b, --backup-id` - Specific backup timestamp (default: latest)

## Prerequisites

Set restic configuration before first use:

```bash
auberge config set restic_repository "rclone:filen:auberge-backup"
auberge config set restic_password "your-encryption-passphrase"
```

Requires `restic` and `rclone` installed locally.

## Examples

```bash
# Push latest backup (auto-selects host if only one exists)
auberge backup push

# Push latest backup for specific host
auberge backup push --host myserver

# Push specific backup by timestamp
auberge backup push --host myserver --backup-id 2026-03-09_14-30-00
```

The repository is automatically initialized on first use.
