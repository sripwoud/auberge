# SSH Keys

Auberge uses a three-tier SSH key resolution system for backup operations, following CLI best practices.

## Resolution Hierarchy

**Precedence**: `--ssh-key` flag > `host.ssh_key` config > default derivation

### Tier 1: Ad-hoc Override (--ssh-key flag)

Use for one-time key overrides:

```bash
auberge backup create --host myhost --ssh-key /path/to/custom/key
```

### Tier 2: Configured Key (hosts.toml)

Configure persistent SSH keys per host:

```bash
# During host creation
auberge host add myhost --address 10.0.0.1 --ssh-key ~/.ssh/custom_key

# Or edit existing host
auberge host edit myhost
```

Configuration stored in: `~/.config/auberge/hosts.toml`

Example:

```toml
[[hosts]]
name = "myhost"
address = "10.0.0.1"
user = "ansible"
port = 22
ssh_key = "~/.ssh/identities/custom_key"
```

### Tier 3: Default Derivation

If no override or config exists, derives path:

- Pattern: `~/.ssh/identities/{user}_{hostname}`
- Example: `~/.ssh/identities/ansible_myhost`

Generate default key:

```bash
auberge ssh keygen --host myhost --user ansible
```

### Interactive Fallback

If the default key doesn't exist, Auberge will:

1. Scan `~/.ssh/` and `~/.ssh/identities/` for available keys
2. Present an interactive selector to choose a key
3. Optionally save your selection to the host configuration

This provides a smooth experience when working with existing keys or migrating infrastructure.

## Importing from SSH Config

When adding a host interactively, you can import from your existing `~/.ssh/config`:

```bash
auberge host add

# If ~/.ssh/config exists, shows:
Found 3 host(s) in ~/.ssh/config

? Import from SSH config or enter manually?:
  > Enter manually
    ansible-old (203.0.113.10)
    myserver (10.0.0.1)
```

Selecting a host from SSH config auto-fills:

- Host name
- Address (HostName)
- User
- Port
- SSH key (IdentityFile)

This eliminates re-entering connection details you already have configured.

## Best Practices

1. **Fresh installs**: Use Tier 3 (default derivation)
2. **Existing infrastructure**: Use Tier 2 (configure once)
3. **Testing/debugging**: Use Tier 1 (ad-hoc override)

## Troubleshooting

### "No SSH keys found"

If you see this error, you have no SSH keys available:

1. Generate a new key: `auberge ssh keygen --host <host>`
2. Or copy existing keys to `~/.ssh/` or `~/.ssh/identities/`

### "Default SSH key not found"

The tool will automatically offer interactive key selection:

1. Shows all available SSH keys in `~/.ssh/` and `~/.ssh/identities/`
2. Select the key you want to use
3. Optionally save it to the host configuration for future use

### "Configured SSH key not found"

- Tool falls back to default derivation with warning
- If default doesn't exist, falls back to interactive selection
- Update config: `auberge host edit <host>`

### Permission warnings

Fix with: `chmod 600 /path/to/key`

## Related Documentation

- [Hosts Configuration](hosts.md) - Managing hosts
- [CLI Reference: ssh keygen](../cli-reference/ssh/keygen.md) - Generate SSH keys
- [CLI Reference: host add](../cli-reference/host/add.md) - Add hosts with SSH keys
