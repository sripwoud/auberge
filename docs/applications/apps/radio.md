# Radio

Continuous streams of your own music, one per station, gated by a single shared password. Icecast2 + Liquidsoap, both from apt. Docs: [icecast.org](https://icecast.org), [liquidsoap.info](https://www.liquidsoap.info)

- **URL**: `https://{subdomain}.{domain}/{station}`
- **Data**: none on the VPS — configs are role-rendered; the station files' canonical copy is your local `~/Music`

## Deploy

Set `radio_listener_password` in your secrets first, then:

```bash
auberge deploy radio
```

## Stations

A station is one `.m3u` file under `~/Music/Stations/`, broadcast at `https://{subdomain}.{domain}/<filename>` (without the `.m3u`). Playlists under `~/Music/Playlists/` stay Navidrome-only — moving a file into `Stations/` is what publishes it (ADR-0020).

Curate with beets and ship with the existing sync — use paths relative to the m3u's location so they resolve both locally and on the VPS:

```bash
beet ls -f '$path' albumartist:Bonobo | sed "s|^$HOME/Music|..|" > ~/Music/Stations/chill.m3u
auberge sync music
```

Editing a station's m3u changes its rotation live. Adding or removing a station file needs `systemctl restart liquidsoap` on the host (or a redeploy), since Liquidsoap discovers stations at startup.

## Notes

Listen in VLC or any player that speaks HTTP audio: open the station URL and authenticate with your admin username and `radio_listener_password`, or inline them:

```
https://{user}:{password}@{subdomain}.{domain}/{station}
```

One credential covers every station; rotating the password revokes all listeners at once. Nothing to back up: redeploying reproduces the app, and stations live in your local music library.
