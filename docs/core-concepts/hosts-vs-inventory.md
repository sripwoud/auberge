# Hosts and Inventory

Auberge uses `hosts.toml` as the single source of truth for host data, with `inventory.yml` providing Ansible group-level configuration.

## Architecture

```
hosts.toml (host data)          inventory.yml (group vars)
├── name, IP, port, user        ├── ansible_user
├── ssh_key                     ├── ansible_ssh_private_key_file
└── tags                        ├── ansible_python_interpreter
                                └── ansible_ssh_common_args
         │                                │
         └──── merged at runtime ─────────┘
                      │
              ansible-playbook -i inventory.yml -i <generated>
```

## hosts.toml (Host Registry)

Personal host registry at `~/.config/auberge/hosts.toml`. Used by all CLI operations: backup, SSH, ansible, DNS.

```toml
[[hosts]]
name = "my-vps"
address = "203.0.113.10"
user = "sripwoud"
port = 59865
ssh_key = "~/.ssh/identities/sripwoud_my-vps"
tags = ["production"]
```

### Management

```bash
auberge host add my-vps 203.0.113.10 --user sripwoud --port 59865
auberge host list
auberge host show my-vps
auberge host edit my-vps
auberge host remove my-vps
```

### Not Version Controlled

This file is user-specific — different users may have different hosts, SSH keys, or ports.

## inventory.yml (Ansible Group Vars)

Version-controlled file at `ansible/inventory.yml`. Contains only group-level variables shared across all hosts — no host-specific entries.

```yaml
all:
  children:
    vps:
      vars:
        ansible_user: ansible
        ansible_ssh_private_key_file: "~/.ssh/identities/ansible_{{ inventory_hostname }}"
        ansible_python_interpreter: /usr/bin/python3
```

When the CLI runs ansible-playbook, it generates a temporary inventory from hosts.toml and merges it with this file. Ansible applies the group vars to the dynamically provided host.

## Usage

```bash
# All operations use hosts.toml as the host source
auberge ansible run --host my-vps
auberge backup create --host my-vps
auberge ssh keygen --host my-vps --user ansible
```

## Adding a New Host

1. Register the host:
   ```bash
   auberge host add my-vps 203.0.113.10 --user root --port 22
   ```

2. Generate SSH keys:
   ```bash
   auberge ssh keygen --host my-vps --user ansible
   ```

3. Bootstrap:
   ```bash
   auberge ansible bootstrap my-vps --ip 203.0.113.10
   ```

## Import from SSH Config

```bash
auberge host add
# If ~/.ssh/config exists, shows importable hosts
```

## Related

- [Hosts Configuration](configuration/hosts.md) - hosts.toml details
- [Ansible Inventory](configuration/ansible-inventory.md) - inventory.yml structure
- [SSH Keys](configuration/ssh-keys.md) - SSH key configuration
