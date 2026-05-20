# First Deployment

## Prerequisites

- Auberge installed (`cargo install auberge`)
- A VPS with root access and an SSH key

## Step 1: Add your VPS

```bash
auberge host add my-vps 203.0.113.10
```

## Step 2: Generate SSH key (optional)

```bash
auberge ssh keygen --host my-vps
```

Key is written to `~/.ssh/identities/root_my-vps`.

## Step 3: Initialize config

```bash
auberge config init --output "$(auberge config path)"
auberge config set hostname my-vps
auberge config set admin_user_name yourname
auberge config set admin_user_email you@example.com
auberge config set ssh_port 22022
```

## Step 4: Bootstrap VPS

!> Configure your VPS provider's firewall to allow your custom SSH port **before** running bootstrap, or you will lose SSH access.

```bash
auberge ansible bootstrap --host my-vps --ip 203.0.113.10
```

Bootstrap creates an admin user, disables root SSH login, and changes the SSH port.

## Step 5: Configure remaining keys

```bash
auberge config set domain example.com
auberge config set cloudflare_dns_api_token your-token
auberge config set baikal_admin_password your-password
auberge config set gokapi_admin_user admin
auberge config set gokapi_admin_password your-password
```

?> Run `auberge config init` (no flags) to print all known config keys.

## Step 6: Deploy full stack

```bash
auberge deploy --all --host my-vps
```

Runs hardening, infrastructure (Caddy, DNS), and all applications. Bootstrap is excluded — it was run in Step 4.

## Step 7: Verify DNS records

```bash
auberge dns status
```

To manually re-sync records: `auberge dns set-all --host my-vps`

## Step 8: Verify services

```bash
ssh user@my-vps 'systemctl status caddy php*-fpm freshrss'
```

## Step 9: Create first backup

```bash
auberge backup create --host my-vps
```

## Troubleshooting

| Symptom                     | Fix                                               |
| --------------------------- | ------------------------------------------------- |
| SSH refused after bootstrap | Allow custom port in VPS provider firewall        |
| Services not starting       | `ssh user@my-vps 'journalctl -u php*-fpm -n 50'`  |
| DNS records missing         | Verify token has Zone:DNS:Edit and Zone:Zone:Read |
