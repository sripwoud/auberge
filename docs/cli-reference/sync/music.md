# auberge sync music

Synchronize music library to remote host

## Synopsis

```bash
auberge sync music [OPTIONS]
```

## Alias

`auberge sy m`

## Description

Synchronizes a local music directory to /srv/music on a remote host using rsync. Designed for Navidrome music streaming server.

Uses --delete flag to mirror local state (removes files on remote that don't exist locally).

## Options

| Option            | Description             | Default               |
| ----------------- | ----------------------- | --------------------- |
| -H, --host HOST   | Target host             | Interactive selection |
| -s, --source PATH | Source music directory  | ~/Music               |
| -n, --dry-run     | Preview without syncing | false                 |

## Examples

```bash
# Sync from default location (interactive host selection)
auberge sync music

# Sync to specific host
auberge sync music --host myserver

# Custom source directory
auberge sync music --host myserver --source /mnt/media/music

# Dry run to preview changes
auberge sync music --host myserver --dry-run
```

## Sync Details

**rsync flags**: `-avzP --delete`

- `-a`: Archive mode (preserves permissions, timestamps, etc.)
- `-v`: Verbose output
- `-z`: Compress during transfer
- `-P`: Show progress and keep partial files
- `--delete`: Remove files on remote that don't exist locally

**Excludes**:

- .DS_Store
- *.tmp

**Target**: /srv/music/ on remote host

**SSH**: Uses ansible user with key from ~/.ssh/identities/ansible_{host}

## Output Example

```
Syncing music to ansible@192.168.1.10:/srv/music/
sending incremental file list
./
Albums/
Albums/Artist - Album/01 - Track.mp3
         3.2M 100%  2.1MB/s    0:00:01
Albums/Artist - Album/02 - Track.mp3
         4.1M 100%  2.3MB/s    0:00:01

sent 125.3M bytes  received 1.2K bytes  8.1M bytes/sec
total size is 8.2G  speedup is 65.47
✓ Music sync completed
```

## Prerequisites

1. **SSH key exists**:
   ```bash
   auberge ssh keygen --host myserver --user ansible
   ```

2. **Ansible user has access** to /srv/music:
   - Created by Navidrome Ansible role
   - Owned by navidrome:navidrome
   - Ansible user needs write access

3. **Navidrome installed** (optional but recommended):
   ```bash
   auberge ansible run --host myserver --tags navidrome
   ```

## Directory Structure

Local:

```
~/Music/
├── Albums/
│   ├── Artist - Album/
│   │   ├── 01 - Track.mp3
│   │   └── cover.jpg
│   └── ...
└── Playlists/
```

Remote (after sync):

```
/srv/music/
├── Albums/
│   ├── Artist - Album/
│   │   ├── 01 - Track.mp3
│   │   └── cover.jpg
│   └── ...
└── Playlists/
```

## Navidrome Integration

After sync, Navidrome will automatically:

- Detect new files
- Update library index
- Extract metadata
- Generate thumbnails

To force rescan:

```bash
ssh ansible@host 'sudo systemctl restart navidrome'
```

## Performance Tips

**First sync (large library)**:

- Use `--dry-run` first to estimate
- Consider compression impact on CPU
- May take hours for multi-GB libraries

**Incremental syncs** (subsequent):

- Much faster (only transfers changes)
- Rsync checksums detect modified files

**Network considerations**:

- Use wired connection for large transfers
- Consider bandwidth limits
- Progress is shown with -P flag

## Troubleshooting

**SSH key not found**:

```bash
SSH key not found: ~/.ssh/identities/ansible_myserver
Run 'auberge ssh keygen --host myserver --user ansible' first
```

Fix: Generate SSH key as shown

**Permission denied on remote**:

- Check ansible user has write access to /srv/music
- Verify with: `ssh ansible@host 'touch /srv/music/test && rm /srv/music/test'`

**Source directory not found**:

- Verify source path exists
- Use absolute path or ensure relative to current directory

**rsync fails**:

- Check network connectivity
- Verify disk space on remote: `ssh ansible@host 'df -h /srv'`
- Check file permissions

## Related Commands

- [auberge backup create](../backup/create.md) - Backup with --include-music
- [auberge ansible run](../ansible/run.md) - Install Navidrome

## See Also

- [Navidrome](../../applications/navidrome.md)
- [Backup Music](../../backup-restore/README.md#music-files)
