# Hosts & Inventory

`hosts.toml` is the single source of truth for host data. `inventory.yml` provides Ansible group-level vars only.

| File            | Path                           | Scope                                                                               | Version controlled |
| --------------- | ------------------------------ | ----------------------------------------------------------------------------------- | ------------------ |
| `hosts.toml`    | `~/.config/auberge/hosts.toml` | Per-user host registry (name, IP, user, port, SSH key, tags) — used by every CLI op | ❌                 |
| `inventory.yml` | `ansible/inventory.yml`        | Shared Ansible group vars (`ansible_user`, `ansible_python_interpreter`, …)         | ✅                 |

At runtime the CLI generates a temporary inventory from `hosts.toml` and merges it with `inventory.yml`.

## hosts.toml

```toml
[[hosts]]
name = "my-vps"
address = "203.0.113.10"
user = "sripwoud"
port = 59865
ssh_key = "~/.ssh/identities/sripwoud_my-vps"
tags = ["production"]
```

Manage with: `auberge host {add,list,show,edit,remove}`.

## inventory.yml

```yaml
all:
  children:
    vps:
      vars:
        ansible_user: ansible
        ansible_ssh_private_key_file: "{{ lookup('env', 'HOME') }}/.ssh/identities/ansible_{{ inventory_hostname }}"
        ansible_python_interpreter: /usr/bin/python3
```

## Adding a new host

```bash
auberge host add my-vps 203.0.113.10 --user root --port 22
auberge ssh keygen --host my-vps --user ansible
auberge ansible bootstrap my-vps --ip 203.0.113.10
```

?> `auberge host add` (no args) detects `~/.ssh/config` and offers to import existing hosts.
