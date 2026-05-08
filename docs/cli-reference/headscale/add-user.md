# auberge headscale add-user

Create a user and generate a pre-auth key

## Synopsis

```bash
auberge headscale add-user [NAME] [OPTIONS]
```

## Alias

`auberge hs au`

## Description

Creates a Headscale user and immediately generates a pre-auth key for that user. The pre-auth key can be passed to `tailscale up --authkey` to connect nodes to your Headscale control plane without manual approval.

When run without arguments, prompts interactively for username and expiration.

## Options

| Option             | Description                                | Default            |
| ------------------ | ------------------------------------------ | ------------------ |
| `[NAME]`           | Username to create                         | Interactive prompt |
| `-e, --expiration` | Pre-auth key expiration (1h, 24h, 48h, 7d) | 24h                |
| `--host HOST`      | Target host running headscale              | Interactive prompt |

## Examples

```bash
# Interactive mode
auberge hs au

# Create user with all args
auberge hs au alice --expiration 48h --host myserver
```

## Output

Prints the pre-auth key and shareable setup instructions for iOS/Tailscale clients.
