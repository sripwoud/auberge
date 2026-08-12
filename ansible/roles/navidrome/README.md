# Navidrome Ansible Role

Installs and configures Navidrome, a self-hosted music streaming server.

## Requirements

- Debian/Ubuntu based system
- Caddy web server (for reverse proxy)
- ffmpeg (installed automatically for transcoding)

## Role Variables

Available variables are listed below, along with default values (see `defaults/main.yml`):

```yaml
navidrome_sys_user: navidrome
navidrome_sys_group: navidrome
navidrome_port: 4533
navidrome_domain: music.sripwoud.xyz
navidrome_music_folder: /srv/music
navidrome_log_level: info
navidrome_scanner_interval: 24h
navidrome_enable_transcoding_config: false
navidrome_enable_downloads: true
navidrome_enable_sharing: true
```

`navidrome_enable_transcoding_config` maps to Navidrome's `EnableTranscodingConfig`, which controls
whether transcoding profiles are editable from the web UI — not whether transcoding happens.
Profiles are shell command templates executed by the server, so an editable profile is an arbitrary
command execution path for any authenticated admin session. Left `false`, matching the upstream
default. On-the-fly transcoding (FLAC to opus, client `maxBitRate` requests, per-player profile and
bitrate selection) is unaffected. Set to `true` only while adding a custom profile, then revert.

## Dependencies

None.

## Example Playbook

```yaml
- hosts: servers
  become: true
  roles:
    - role: navidrome
      vars:
        navidrome_domain: music.example.com
        navidrome_music_folder: /mnt/music
```

## Features

- Installation from official .deb package via GitHub releases
- Automatic dependency management via APT
- On-the-fly transcoding via ffmpeg using Navidrome's built-in profiles
- Systemd service included in package
- Caddy reverse proxy configuration with gzip compression
- Automatic scanning of music library

## Configuration

The role configures Navidrome via `/etc/navidrome/navidrome.toml` with:

- Custom music folder path
- Port configuration
- Download, sharing, and transcoding-profile-editability settings
- Automatic library scanning schedule

## License

MIT

## Author Information

This role was created for self-hosting purposes.
