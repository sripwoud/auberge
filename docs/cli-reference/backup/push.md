# auberge backup push

Push local backups to an offsite restic repository. Alias: `auberge b p`.

```bash
auberge backup push [OPTIONS]
```

The repository is initialized automatically on first push.

?> `push` is upload-only — it never applies the retention policy. [backup sync](cli-reference/backup/sync.md) is the retention-enforcing entrypoint (it runs prune after push); standalone pushes accumulate snapshots until the next `sync` or manual [backup prune](cli-reference/backup/prune.md).

## Options

| Option               | Description               | Default                         |
| -------------------- | ------------------------- | ------------------------------- |
| `-H, --host HOST`    | Filter backups by host    | Interactive (if multiple hosts) |
| `-b, --backup-id ID` | Specific backup timestamp | latest                          |

## Setup

Before first use, configure rclone and restic:

```bash
# 1. Install restic and rclone (>= v1.73 required for native Filen backend)
brew install restic rclone   # or: sudo apt install restic rclone

# 2. Configure rclone remote (follow interactive wizard)
rclone config   # create remote named e.g. "filen", type: filen
rclone mkdir filen:auberge-backup

# 3. Set auberge config
auberge config set restic_repository "rclone:filen:auberge-backup"
auberge config set restic_password "your-encryption-passphrase"
```

?> `restic_password` is NOT your Filen password — it is restic's own encryption key. Store it in a password manager; losing it means losing access to all snapshots. Both values support `!` command syntax: `auberge config set restic_password '!pass show auberge/restic'`.

## Examples

```bash
auberge backup push
auberge backup push --host myserver
auberge backup push --host myserver --backup-id 2026-03-09_14-30-00
```
