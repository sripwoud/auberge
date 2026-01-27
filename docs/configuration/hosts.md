# Hosts Configuration

Hosts can be managed in two ways depending on your use case.

## XDG Config (Recommended for end users)

For users installing via `cargo install`, hosts are managed in `~/.config/auberge/hosts.toml`:

```bash
# Add a host interactively
auberge host add my-vps

# Or non-interactively
auberge host add my-vps 194.164.53.11 --user admin --port 22

# List hosts
auberge host list

# Show host details
auberge host show my-vps

# Edit host
auberge host edit my-vps

# Remove host
auberge host remove my-vps
```

The `hosts.toml` format:

```toml
[[hosts]]
name = "auberge"
address = "194.164.53.11"
user = "sripwoud"
port = 22
tags = ["production"]
description = "Main VPS"
ssh_key = "~/.ssh/identities/sripwoud_auberge"
```

## Ansible Inventory (Recommended for developers)

For development, keep using `ansible/inventory.yml` in the repository:

```yaml
all:
  children:
    vps:
      hosts:
        auberge:
          ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
          ansible_port: 22
          bootstrap_user: root
```

## Priority Order

The CLI checks hosts in this order:

1. `~/.config/auberge/hosts.toml` (if exists and not empty)
2. `ansible/inventory.yml` (fallback for developers)
3. Environment variables (legacy support)

## hosts.toml vs inventory.yml

- **hosts.toml**: User-specific hosts (backup operations)
  - Location: `~/.config/auberge/hosts.toml`
  - Not version controlled
  - Managed via `auberge host` commands

- **inventory.yml**: Ansible playbooks (shared infrastructure)
  - Location: `ansible/inventory.yml`
  - Version controlled
  - Used by `auberge ansible` commands

## Related Documentation

- [SSH Keys](ssh-keys.md) - SSH key management
- [CLI Reference: host](../cli-reference/host/add.md) - Host management commands
