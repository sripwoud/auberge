# auberge host add

Add host to configuration

## Usage

```bash
auberge host add [NAME] [ADDRESS] [OPTIONS]
```

## Options

- `-u, --user` - SSH user (default: current user or root)
- `-p, --port` - SSH port (default: 22)
- `--ssh-key` - Path to SSH key
- `-t, --tags` - Tags (comma-separated)
- `-d, --description` - Description
- `--no-input` - Disable interactive prompts

## Examples

```bash
# Interactive (scans SSH config)
auberge host add

# Non-interactive
auberge host add myserver 192.168.1.10 --user ansible --port 2222 --no-input

# With tags
auberge host add prod 10.0.0.5 --tags production,web
```

Use `auberge host add --help` for full options.
