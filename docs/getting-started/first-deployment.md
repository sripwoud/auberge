# First Deployment

Detailed walkthrough of deploying Auberge for the first time.

## Prerequisites

- Auberge installed (`cargo install auberge`)
- A VPS with root access
- SSH key for VPS access
- (Optional) Cloudflare account for DNS

## Step 1: Add Your VPS

Add your VPS as a host:

```bash
auberge host add my-vps 203.0.113.10
```

You'll be prompted for:

- **SSH user**: Usually `root` for initial setup
- **SSH port**: Default is `22`
- **SSH key**: Optional, defaults to `~/.ssh/identities/{user}_{hostname}`

## Step 2: Generate SSH Key (Optional)

If you don't have an SSH key:

```bash
auberge ssh keygen --host my-vps
```

This creates a key at `~/.ssh/identities/root_my-vps`.

## Step 3: Bootstrap VPS

First-time VPS setup (creates admin user, secures SSH):

```bash
auberge ansible bootstrap --host my-vps --ip 203.0.113.10
```

This:

- Creates an admin user
- Disables root SSH login
- Changes SSH port (from `SSH_PORT` env var)
- Configures basic security

**Important**: Configure your VPS provider's firewall to allow your custom SSH port before running bootstrap.

## Step 4: Configure Environment

Set required environment variables:

```bash
mise set --age-encrypt --prompt PRIMARY_DOMAIN
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
mise set --age-encrypt --prompt BAIKAL_ADMIN_PASSWORD
mise set --age-encrypt --prompt WEBDAV_PASSWORD
```

See [Environment Variables](../configuration/environment-variables.md) for all options.

## Step 5: Deploy Full Stack

```bash
auberge ansible run
```

Select:

1. **Host**: `my-vps`
2. **Playbook**: `playbooks/auberge.yml` (complete stack)

This runs all layers:

- Bootstrap (if not done)
- Hardening (firewall, fail2ban)
- Infrastructure (Caddy, DNS)
- Applications (all services)

## Step 6: Configure DNS

Point your DNS records to your VPS:

```bash
auberge dns set-all --host my-vps
```

This creates A records for all subdomains:

- `cal.yourdomain.com`
- `rss.yourdomain.com`
- `musique.yourdomain.com`
- etc.

## Step 7: Verify Services

Check that services are running:

```bash
ssh user@my-vps 'systemctl status caddy php*-fpm freshrss'
```

Access services in your browser:

- `https://cal.yourdomain.com`
- `https://rss.yourdomain.com`

## Step 8: Create First Backup

```bash
auberge backup create --host my-vps
```

This backs up all application data locally.

## Troubleshooting

### Can't Connect via SSH After Bootstrap

Your VPS provider's firewall may be blocking the new SSH port. Log into the provider's control panel and allow the custom port.

### Services Not Starting

Check logs:

```bash
ssh user@my-vps 'journalctl -u php*-fpm -n 50'
```

### DNS Records Not Created

Verify Cloudflare API token has correct permissions:

- Zone → DNS → Edit
- Zone → Zone → Read

## Next Steps

- [Backup & Restore](../backup-restore/overview.md) - Regular backups
- [Running Playbooks](../deployment/running-playbooks.md) - Deploy specific services
- [CLI Reference](../cli-reference/auberge.md) - All commands
