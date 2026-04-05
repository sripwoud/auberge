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

## Setup

### Step 1: Install restic and rclone

```bash
# restic
brew install restic          # macOS
sudo apt install restic      # Debian/Ubuntu

# rclone (>= v1.73 required for native Filen backend)
brew install rclone
sudo apt install rclone
```

Verify versions:

```bash
restic version
rclone version  # must be >= 1.73
```

### Step 2: Configure rclone remote for [Filen](https://filen.io)

Run the interactive config wizard:

```bash
rclone config
```

When prompted:

1. Choose `n` to create a new remote
2. Name it (e.g. `filen`)
3. Select `filen` as the storage type
4. Enter your Filen account email and password
5. Accept defaults for the remaining prompts, then quit

Create a folder on Filen to store backups (e.g. `auberge-backup`). You can do this via the Filen web UI or:

```bash
rclone mkdir filen:auberge-backup
```

Verify the remote works:

```bash
rclone ls filen:auberge-backup
```

### Step 3: Set auberge config

```bash
auberge config set restic_repository "rclone:filen:auberge-backup"
auberge config set restic_password "your-encryption-passphrase"
```

- `restic_repository` - the rclone remote path in the format `rclone:<remote-name>:<folder>`. This tells restic where to store snapshots.
- `restic_password` - restic's own encryption passphrase. This is **not** your Filen password — it is a separate key restic uses to encrypt data before uploading. Choose a strong passphrase and store it in a password manager. Losing it means losing access to all snapshots.

Both values support `!` command syntax to fetch from a password manager (see [Secrets Management](../../configuration/secrets.md#password-commands)):

```bash
auberge config set restic_password '!pass show auberge/restic'
```

The repository is initialized automatically on first push.

## Full Workflow

```bash
# 1. Create a local backup
auberge backup create --host myserver

# 2. Push to offsite restic repository
auberge backup push --host myserver

# 3. Apply retention policy and prune old snapshots
auberge backup prune
```

See [backup create](create.md) and [backup prune](prune.md) for full options.

## Examples

```bash
# Push latest backup (auto-selects host if only one exists)
auberge backup push

# Push latest backup for specific host
auberge backup push --host myserver

# Push specific backup by timestamp
auberge backup push --host myserver --backup-id 2026-03-09_14-30-00
```
