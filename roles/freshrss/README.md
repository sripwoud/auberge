# FreshRSS Ansible Role

Deploys FreshRSS RSS reader as a systemd service with Caddy reverse proxy.

## Features

- Installs FreshRSS from official GitHub repository
- Runs as dedicated system user with restricted permissions
- Uses SQLite for simple database management
- Systemd service with security hardening
- Caddy reverse proxy with HTTPS support
- Automatic updates via git pull

## Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `freshrss_install_path` | `/opt/freshrss` | Installation directory |
| `freshrss_sys_user` | `freshrss` | System user |
| `freshrss_sys_group` | `freshrss` | System group |
| `freshrss_port` | `8084` | Local port for PHP server |
| `freshrss_domain` | `rss.sripwoud.xyz` | Domain for Caddy |
| `freshrss_version` | `edge` | Git branch/tag to deploy |
| `freshrss_data_dir` | `/var/lib/freshrss` | Persistent data directory |
| `freshrss_db_type` | `sqlite` | Database type |

## Post-Installation

After deployment, complete the setup via web interface:

1. Navigate to `https://{{ freshrss_domain }}`
2. Follow the installation wizard
3. Select SQLite as database
4. Complete admin account creation

## Management

```bash
# Check service status
sudo systemctl status freshrss

# View logs
sudo journalctl -u freshrss -f

# Restart service
sudo systemctl restart freshrss

# Update FreshRSS
cd /opt/freshrss
sudo -u freshrss git pull
sudo systemctl restart freshrss
```

## Cron Jobs

For automatic feed updates, add to system crontab:

```bash
# Update feeds every 15 minutes
*/15 * * * * freshrss /usr/bin/php /opt/freshrss/app/actualize_script.php > /dev/null 2>&1
```

## Security

The systemd service includes hardening:
- `NoNewPrivileges=true`
- `PrivateDevices=true`
- `PrivateTmp=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `ReadWritePaths` limited to data directories

## Backup

Important directories to backup:
- `/var/lib/freshrss` - User data and SQLite database
- `/opt/freshrss/data` - Configuration and cache

## Dependencies

- Caddy reverse proxy must be installed
- PHP 8.0+ with required extensions
