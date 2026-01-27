# Navidrome

Modern music streaming server compatible with Subsonic/Airsonic clients.

Official Documentation: [https://navidrome.org](https://navidrome.org)

## Deployment

```bash
auberge ansible run --tags navidrome
```

## Music Sync

Sync music files to VPS:

```bash
auberge sync music --host my-vps --source ~/Music
```

See [CLI Reference: sync music](../../cli-reference/sync/music.md).

## Backup

Backed up by default (database and configuration only). Music files excluded by default to save space.

Include music in backup:

```bash
auberge backup create --include-music
```

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Backup & Restore](../../backup-restore/overview.md)
- [CLI Reference: sync music](../../cli-reference/sync/music.md)
- [Applications Overview](../overview.md)
