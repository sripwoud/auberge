# YOURLS Ansible Role

Deploys and configures YOURLS (Your Own URL Shortener) with MariaDB backend, PHP-FPM, and Caddy reverse proxy.

## Requirements

- Debian/Ubuntu system
- Caddy web server
- Root or sudo access

## Role Variables

### Required Environment Variables

The following environment variables must be set before running the role:

- `YOURLS_DB_PASSWORD` - Database password for YOURLS user (required)
- `YOURLS_ADMIN_PASSWORD` - Admin password for YOURLS interface (required)
- `YOURLS_COOKIEKEY` - Random string for cookie encryption (required)
- `YOURLS_API_SIGNATURE` - Random string for API authentication (required)

### Optional Environment Variables

- `YOURLS_DB_NAME` - Database name (default: `yourls`)
- `YOURLS_DB_USER` - Database username (default: `yourls`)
- `YOURLS_DB_PREFIX` - Database table prefix (default: `yourls_`)
- `YOURLS_SITE_TITLE` - Site title displayed in admin (default: `URL Shortener`)
- `YOURLS_ADMIN_USER` - Admin username (default: `admin`)

### Default Variables

See `defaults/main.yml`:

```yaml
yourls_install_path: /var/www/yourls
yourls_sys_user: www-data
yourls_sys_group: www-data
yourls_version: "1.10.2"
yourls_db_host: localhost
```

## What This Role Does

1. **Installs dependencies**: PHP, PHP-FPM, MariaDB, required PHP extensions
2. **Detects PHP version**: Automatically discovers installed PHP-FPM version
3. **Database setup**: Creates database, user, and grants privileges
4. **Clones YOURLS**: Fetches official YOURLS repository from GitHub
5. **Configuration**: Deploys templated `config.php` with environment-based credentials
6. **Database initialization**: Automatically creates tables and initializes options on first run
7. **Caddy integration**: Configures reverse proxy with:
   - Root redirect to `/admin/index.php`
   - Static asset serving (CSS, JS, images)
   - PHP-FPM integration via Unix socket
   - Proper URL rewriting to `yourls-loader.php`

## Deployment

### Example Playbook

```yaml
- hosts: webservers
  roles:
    - role: yourls
      vars:
        yourls_subdomain: s
        primary_domain: example.com
```

### Environment Variables Example

```bash
export YOURLS_DB_PASSWORD="$(openssl rand -base64 32)"
export YOURLS_ADMIN_PASSWORD="strong_password_here"
export YOURLS_COOKIEKEY="$(openssl rand -base64 32)"
export YOURLS_API_SIGNATURE="$(openssl rand -base64 32)"
```

Run playbook:

```bash
ansible-playbook -i inventory playbook.yml
```

## Architecture

- **Installation path**: `/var/www/yourls`
- **Database**: MariaDB on `localhost` via Unix socket
- **Web server**: Caddy with PHP-FPM via `/var/run/php/phpX.X-fpm.sock`
- **Domain**: `{{ yourls_subdomain }}.{{ primary_domain }}`
- **Admin interface**: `https://{{ yourls_domain }}/admin/`

## Idempotency

The role is fully idempotent:

- Skips cloning if repository exists
- Checks for local modifications before updating
- Initializes database only if tables or options are missing
- Does not overwrite existing configurations on subsequent runs

## Troubleshooting

### Database initialization fails

Check that MariaDB is running:

```bash
systemctl status mysql
```

Verify database connection:

```bash
mysql -u yourls -p -h localhost yourls
```

### PHP-FPM not processing requests

Verify PHP-FPM is running:

```bash
systemctl status php*-fpm
```

Check Caddy can access the socket:

```bash
ls -l /var/run/php/php*-fpm.sock
```

Ensure `www-data` group membership:

```bash
groups caddy
```

### Admin page shows "YOURLS is not installed"

Database tables may not be initialized. Check:

```bash
mysql -u yourls -p -D yourls -e "SHOW TABLES;"
```

If empty, the initialization task should have run. Check Ansible output for errors.

### Static assets (CSS/JS) not loading

Verify Caddyfile has `@static` matcher and `file_server` directive. Check permissions:

```bash
ls -la /var/www/yourls
```

## License

MIT

## Author

Deployed via Ansible as part of auberge infrastructure
