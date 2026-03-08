# Ansible Inventory

Location: `ansible/inventory.yml`

Defines group-level variables for Ansible playbook execution. Host-specific data (IPs, ports) is sourced from `~/.config/auberge/hosts.toml` and injected as a dynamic inventory at runtime.

## Structure

```yaml
all:
  children:
    vps:
      vars:
        ansible_user: ansible
        ansible_ssh_private_key_file: "{{ lookup('env', 'HOME') }}/.ssh/identities/ansible_{{ inventory_hostname }}"
        admin_ssh_private_key_file: "{{ lookup('env', 'HOME') }}/.ssh/identities/{{ admin_user_name }}_{{ inventory_hostname }}"
        ansible_python_interpreter: /usr/bin/python3
        ansible_ssh_pipelining: true
```

## How It Works

When running `auberge ansible run`, the CLI:

1. Reads hosts.toml for the target host's IP, port, and user
2. Generates a temporary inventory YAML with the host entry
3. Passes both inventories to Ansible: `-i inventory.yml -i <generated>`
4. Ansible merges them: group vars from static file + host data from generated file

## Group Variables

- `ansible_user` - SSH user after bootstrap (`ansible`)
- `ansible_ssh_private_key_file` - Path to SSH key (pattern-based per host)
- `admin_ssh_private_key_file` - Admin user's SSH key path
- `ansible_python_interpreter` - Python 3 path
- `ansible_ssh_pipelining` - Enable SSH pipelining
- `ansible_ssh_common_args` - SSH connection tuning options

## Adding a Host

Hosts are managed via `hosts.toml`, not the inventory file:

```bash
auberge host add my-host 203.0.113.10 --user root --port 22
auberge ssh keygen --host my-host --user ansible
auberge ansible bootstrap my-host --ip 203.0.113.10
```

## Troubleshooting

**"Host not found"**

```bash
auberge host list
auberge host add my-host 203.0.113.10
```

**"SSH key not found"**

```bash
auberge ssh keygen --host my-host --user ansible
```

## Related

- [Hosts Configuration](./hosts.md) - hosts.toml management
- [SSH Keys](./ssh-keys.md) - SSH key configuration
