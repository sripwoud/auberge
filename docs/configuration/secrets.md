# Secrets Management

Auberge uses [age](https://github.com/FiloSottile/age) encryption via mise for secrets (SSH ports, API tokens, passwords, IPs).

## Setup

```bash
# Install age
brew install age  # macOS
apt install age   # Debian/Ubuntu

# Generate key
age-keygen -o ~/.config/age/key.txt
```

**Back up `~/.config/age/key.txt` - losing this means losing access to secrets.**

## Setting Secrets

```bash
mise set --age-encrypt --prompt SECRET_NAME
```

**Required:**

```bash
mise set --age-encrypt --prompt ADMIN_USER_NAME
mise set --age-encrypt --prompt ADMIN_USER_EMAIL
mise set --age-encrypt --prompt PRIMARY_DOMAIN
mise set --age-encrypt --prompt CLOUDFLARE_DNS_API_TOKEN
mise set --age-encrypt --prompt SSH_PORT
mise set --age-encrypt --prompt AUBERGE_HOST
```

See [Environment Variables](configuration/environment-variables.md) for complete list.

## Usage

```bash
mise env                    # List all (decrypted)
mise env | grep SSH_PORT    # View specific secret
```

Secrets are auto-decrypted by mise and accessed by Ansible via `lookup('env', 'VAR')`.

## Security

- Back up `~/.config/age/key.txt` securely
- `chmod 600 ~/.config/age/key.txt`
- Never commit plaintext secrets
- Encrypted `mise.toml` is safe to commit
- Each team member should use their own age key

## Troubleshooting

**"Environment variable not set"**

```bash
mise env | grep SECRET_NAME  # Check if exists
mise set --age-encrypt --prompt SECRET_NAME  # Set if missing
```

**"no identity matched"**

```bash
ls -l ~/.config/age/key.txt  # Verify key exists
```
