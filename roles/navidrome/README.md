# Navidrome Ansible Role

Installs and configures Navidrome, a self-hosted music streaming server.

## Requirements

- Debian/Ubuntu based system
- Caddy web server (for reverse proxy)
- ffmpeg (installed automatically for transcoding)

## Role Variables

Available variables are listed below, along with default values (see `defaults/main.yml`):

```yaml
navidrome_install_path: /opt/navidrome
navidrome_sys_user: navidrome
navidrome_sys_group: navidrome
navidrome_port: 4533
navidrome_domain: music.sripwoud.xyz
navidrome_data_dir: /var/lib/navidrome
navidrome_music_folder: /srv/music
navidrome_log_level: info
navidrome_scanner_interval: 24h
navidrome_enable_transcoding: true
navidrome_enable_downloads: true
navidrome_enable_sharing: true
```

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

- Binary installation from GitHub releases with checksum verification
- SQLite database for metadata storage
- Transcoding support with ffmpeg
- Hardened systemd service with security restrictions
- Caddy reverse proxy configuration with gzip compression
- Automatic scanning of music library

## Security Hardening

The systemd service includes the following security features:

- NoNewPrivileges
- PrivateDevices
- PrivateTmp
- ProtectSystem=strict
- ProtectHome
- ProtectKernelTunables
- ProtectKernelModules
- Limited read/write access to data and music directories

## License

MIT

## Author Information

This role was created for self-hosting purposes.
