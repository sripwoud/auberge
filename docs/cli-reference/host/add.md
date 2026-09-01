# auberge host add

Add host to configuration

## Usage

```bash
auberge host add [NAME] [ADDRESS] [OPTIONS]
# Alias: auberge h a
```

## Options

- `-u, --user` - SSH user (default: current user or root)
- `-p, --port` - SSH port (default: 22)
- `--ssh-key` - Path to SSH key
- `-t, --tags` - Tags (comma-separated)
- `-d, --description` - Description
- `--tailnet-tag` - Tailnet trust tier: `trusted`, `data`, `agent`, or `standby` (ADR-0055). Prompted interactively; omit to leave unset
- `--no-input` - Disable interactive prompts

## Examples

```bash
# Interactive (scans SSH config)
auberge host add

# Non-interactive
auberge host add myserver 192.168.1.10 --user ansible --port 2222 --no-input

# With tags
auberge host add prod 10.0.0.5 --tags production,web

# With a trust tier — separate from tags, which are Ansible inventory groups
auberge host add ruche 203.0.113.7 --tailnet-tag agent --no-input
```

> [!IMPORTANT]
> `--tags` and `--tailnet-tag` are different axes. `--tags` become Ansible
> inventory groups (`when: "'hermes' in group_names"`); `--tailnet-tag` is the
> host's ACL trust tier. Adding a tag never changes what a host may reach on the
> tailnet.

Use `auberge host add --help` for full options.
