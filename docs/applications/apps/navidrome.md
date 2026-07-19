# Navidrome

Music streaming server compatible with Subsonic/Airsonic clients. Docs: [navidrome.org](https://navidrome.org)

- **URL**: `https://{subdomain}.{domain}`
- **Data**: database and config on VPS; music files at configured path

## Deploy

```bash
auberge deploy navidrome
```

## Notes

Sync music files to the VPS:

```bash
auberge sync music --host my-vps --source ~/Music
```

Backed up by default (database and config only; music excluded to save space). To include music: `auberge backup create --include-music`. See [Backup & Restore](backup-restore/overview.md).
