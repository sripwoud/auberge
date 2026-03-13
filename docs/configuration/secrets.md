# Secrets Management

Auberge stores all configuration, including sensitive values, in `config.toml`.

## Setup

Initialize the config file at the XDG config path:

```bash
auberge config init
```

## Setting Config Values

```bash
auberge config set SECRET_NAME value
```

**Required values:**

```bash
auberge config set hostname yourserver
auberge config set admin_user_name yourname
auberge config set admin_user_email you@example.com
auberge config set domain example.com
auberge config set cloudflare_dns_api_token your-token
auberge config set ssh_port 22022
```

**Optional values:**

```bash
# Only needed for Cockpit web console login
auberge config set admin_user_password your-linux-password
```

See `config.example.toml` for the complete list.

## Viewing Config

```bash
auberge config list
```

## Security

- Never commit `config.toml` to version control - it contains plaintext secrets
- `config.toml` is listed in `.gitignore`
- Use `config.example.toml` (committed) as a reference for required keys

## Troubleshooting

**"Missing required config value"**

```bash
auberge config list              # Check what is set
auberge config set KEY value     # Set if missing
```
