# Radio Ansible Role

Installs and configures the Radio: Icecast2 and Liquidsoap, both from apt,
broadcasting each `.m3u` file under the stations directory as one continuous,
password-gated stream (ADR-0020).

## Requirements

- Debian/Ubuntu based system
- Caddy web server (for reverse proxy and listener auth)
- Music library synced to the host (`auberge sync music`)

## Role Variables

Available variables are listed below, along with default values (see `defaults/main.yml`):

```yaml
radio_icecast_port: 8005
radio_config_dir: /etc/liquidsoap
radio_stations_dir: /srv/music/Stations
radio_music_group: navidrome
radio_source_password_file: /etc/icecast2/source_password
radio_bitrate: 128
radio_max_clients: 32
radio_max_sources: 8
```

`radio_icecast_port` defaults to 8005 because 8000, Icecast's usual port, is taken by
Paperless-ngx's webserver on this host layout.

Required from the Key Registry: `radio_subdomain`, `radio_listener_password`,
`admin_user_name`, `domain`, `cloudflare_dns_api_token`.

## Behaviour

- Liquidsoap and Icecast2 both install from the Debian archive; there is no version
  pin and no upstream download (ADR-0037). Savonet's trixie release debs declare their
  ffmpeg dependencies at deb-multimedia's epoch 10, which Debian's epoch-7 ffmpeg can
  never satisfy — the asset is uninstallable on a stock trixie, whatever the version.
  Trade-off: trixie's 2.3.2 over upstream's newer releases, with security fixes
  arriving through `trixie-security` like every other apt package.
- A Station is one `.m3u` under `radio_stations_dir`, served at
  `https://{{ radio_domain }}/<filename-without-extension>`.
- Liquidsoap globs `*.m3u` at startup: adding or removing a station file needs
  `systemctl restart liquidsoap` (or a redeploy); editing an existing file's content is
  picked up live (`reload_mode="watch"`).
- Icecast binds 127.0.0.1 only. Every listener reaches it through Caddy `basic_auth`
  with one shared credential, hashed at deploy — the `colporteur` pattern.
- The Icecast source password is host-internal: generated once into
  `radio_source_password_file`, never registered as a key.
- `/admin/*`, `/status.xsl`, `/status-json.xsl` and `/server-version.xsl` are blocked at
  Caddy so the mount list leaks to nobody, authenticated or not.
- Memory Budgets for the `liquidsoap` and `icecast2` units are declared in
  `ansible/playbooks/radio.meta.yml` and injected at deploy (ADR-0021). Icecast's unit is
  generated from the apt-shipped init script, so its budget lands as a systemd drop-in.
- The unit sets `OCAMLRUNPARAM=o=40`: OCaml's default GC keeps ~120% heap slack, which
  measured 448M anonymous for two stations; bounding slack at 40% cut that to 164M with
  both mounts still serving (#481). The budgets are sized from the tuned figure.
- The unit points both liquidsoap cache dirs (`LIQ_CACHE_USER_DIR`,
  `LIQ_CACHE_SYSTEM_DIR`) at its `CacheDirectory` (`/var/cache/liquidsoap`). The
  defaults — `$HOME/.cache/liquidsoap` and `/usr/share/liquidsoap-lang/cache` — are
  both unwritable under `ProtectHome` + `ProtectSystem=strict`, which cost a full
  script recompile and a read-only-filesystem error log on every start.
- The encoder is plain `%mp3(bitrate=)`. The `pcm_s16` frame request (16-bit buffered
  audio, 4x less memory per buffered second) is a 2.4 parameter: 2.3.2 rejects it at
  encoder instantiation — at runtime, only once a station exists, and invisibly to
  `liquidsoap --check` (ADR-0037). Re-add it when the archive ships 2.4.

## Dependencies

None.

## Example Playbook

```yaml
- hosts: vps
  become: true
  roles:
    - role: radio
      tags: [apps, media, radio]
```
