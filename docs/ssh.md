# SSH Configuration

## Overview

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

## Best Practices

1. **Fresh installs**: Use Tier 3 (default derivation)
2. **Existing infrastructure**: Use Tier 2 (configure once)
3. **Testing/debugging**: Use Tier 1 (ad-hoc override)

## Troubleshooting

### "SSH key not found"

1. Check if key exists: `ls -la ~/.ssh/identities/`
2. Generate missing key: `auberge ssh keygen --host <host>`
3. Or configure custom key: `auberge host edit <host>`

### "Configured SSH key not found"

- Tool falls back to default derivation with warning
- Update config: `auberge host edit <host>`

### Permission warnings

Fix with: `chmod 600 /path/to/key`

## hosts.toml vs inventory.yml

- **hosts.toml**: User-specific hosts (backup operations)
  - Location: `~/.config/auberge/hosts.toml`
  - Not version controlled
  - Managed via `auberge host` commands

- **inventory.yml**: Ansible playbooks (shared infrastructure)
  - Location: `ansible/inventory.yml`
  - Version controlled
  - Used by `auberge ansible` commands
