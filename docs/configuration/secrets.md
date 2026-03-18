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

## Password Commands

Values prefixed with `!` are treated as shell commands. At deploy time, Auberge runs the command via `sh -c` and uses the trimmed stdout as the actual value. The command is **not** evaluated at `config set` time.

**Value formats:**

| Stored value   | Behaviour                                               |
| -------------- | ------------------------------------------------------- |
| `!cmd`         | Runs `sh -c "cmd"`, uses trimmed stdout                 |
| `!!literal`    | Stores as `!literal` (escape hatch - no command is run) |
| `plain string` | Used as-is                                              |

**Example (`config.toml`):**

```toml
vdirsyncer_icloud_password = "!pass icloud/app-password"
baikal_admin_password = "!op read op://vault/baikal/password"
cloudflare_dns_api_token = "secret123"
some_literal_bang = "!!not-a-cmd"
```

**Requirements:** the command must exit 0 and produce non-empty UTF-8 output. Deployment fails with a clear error if either condition is not met.

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
