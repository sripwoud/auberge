# auberge sync music

Synchronize a local music directory to `/srv/music` on a remote host. Alias: `auberge sy m`.

```bash
auberge sync music [OPTIONS]
```

Uses `rsync -rltz --partial --delete --delete-excluded`, mirroring local state (remote files not present locally are deleted). Excludes hidden entries (`.*`) and `*.tmp` at any depth — everything else transfers, including cover art, booklets, liner notes and `.m3u` playlists. SSH key: `~/.ssh/identities/ansible_{host}`.

## Options

| Option              | Description             | Default     |
| ------------------- | ----------------------- | ----------- |
| `-H, --host HOST`   | Target host             | Interactive |
| `-s, --source PATH` | Source music directory  | `~/Music`   |
| `-n, --dry-run`     | Preview without syncing | false       |

## Examples

```bash
auberge sync music --host myserver
auberge sync music --host myserver --source /mnt/media/music
auberge sync music --host myserver --dry-run
```

## Gotchas

- `--delete` removes remote files that no longer exist locally — run `--dry-run` before the first sync of a large library.
- Hidden entries are excluded as a class, not per tool: a `.memsearch/`, `.git/` or `.cache/` left in the library by tooling never reaches Navidrome, and `--delete-excluded` removes any copy an earlier sync already made. A file you _want_ on the host must not be hidden.
- Ansible user requires write access to `/srv/music` (created by the Navidrome Ansible role). Verify: `ssh ansible@host 'touch /srv/music/test && rm /srv/music/test'`.
- Missing SSH key: run `auberge ssh keygen --host myserver --user ansible` first.
- To force a Navidrome rescan: `ssh ansible@host 'sudo systemctl restart navidrome'`.
