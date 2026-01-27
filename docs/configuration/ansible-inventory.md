# Ansible Inventory

Location: `ansible/inventory.yml`

Defines deployment targets and connection parameters.

## Structure

```yaml
all:
  children:
    vps:
      hosts:
        auberge:
          ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
          ansible_port: "{{ lookup('env', 'SSH_PORT') }}"
          bootstrap_user: root
      vars:
        ansible_user: ansible
        ansible_ssh_private_key_file: "{{ lookup('env', 'HOME') }}/.ssh/identities/ansible_{{ inventory_hostname }}"
        ansible_python_interpreter: /usr/bin/python3
```

## Key Variables

**Per-host:**

- `ansible_host` - IP/hostname (from env var)
- `ansible_port` - SSH port (from env var)
- `bootstrap_user` - Initial user (usually `root`)

**Group vars (vps):**

- `ansible_user` - SSH user after bootstrap (`ansible`)
- `ansible_ssh_private_key_file` - Path to SSH key
- `ansible_python_interpreter` - Python 3 path

## Environment Variables

Uses `lookup('env', 'VAR')` for sensitive data:

```bash
mise set --age-encrypt --prompt AUBERGE_HOST
mise set --age-encrypt --prompt SSH_PORT
```

Benefits:

- Inventory file can be version controlled
- Actual values encrypted in `mise.toml`
- Per-environment/per-user configurations

## Adding a Host

1. Edit `ansible/inventory.yml`:

```yaml
my-host:
  ansible_host: "{{ lookup('env', 'MY_HOST') }}"
  ansible_port: "{{ lookup('env', 'SSH_PORT') }}"
  bootstrap_user: root
```

2. Set env var and generate keys:

```bash
mise set --age-encrypt --prompt MY_HOST
auberge ssh keygen --host my-host --user ansible
```

3. Bootstrap:

```bash
auberge ansible bootstrap my-host --ip 10.0.0.1
```

## Validation

```bash
ansible-inventory -i ansible/inventory.yml --list
ansible-inventory -i ansible/inventory.yml --host auberge
```

## Troubleshooting

**"environment variable not set"**

```bash
mise env | grep AUBERGE_HOST
mise set --age-encrypt --prompt AUBERGE_HOST  # If missing
```

**"SSH key not found"**

```bash
auberge ssh keygen --host auberge --user ansible
```
