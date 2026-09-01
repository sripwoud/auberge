# Hosts & Inventory

`hosts.toml` is the single source of truth for host data. `inventory.yml` provides Ansible group-level vars only.

| File            | Path                           | Scope                                                                                             | Version controlled |
| --------------- | ------------------------------ | ------------------------------------------------------------------------------------------------- | ------------------ |
| `hosts.toml`    | `~/.config/auberge/hosts.toml` | Per-user host registry (name, IP, user, port, SSH key, tags, tailnet tier) — used by every CLI op | ❌                 |
| `inventory.yml` | `ansible/inventory.yml`        | Shared Ansible group vars (`ansible_user`, `ansible_python_interpreter`, …)                       | ✅                 |

At runtime the CLI generates a temporary inventory from `hosts.toml` and merges it with `inventory.yml`.

## hosts.toml

```toml
[[hosts]]
name = "my-vps"
address = "203.0.113.10"
user = "sripwoud"
port = 59865
ssh_key = "~/.ssh/identities/my-vps/sripwoud"
tags = ["production"]
tailnet_tag = "data"
```

`tags` become Ansible inventory groups; `tailnet_tag` is the ACL trust tier and never reaches Ansible (ADR-0062).

Manage with: `auberge host {add,list,show,edit,remove}`.

## inventory.yml

```yaml
all:
  children:
    vps:
      vars:
        ansible_user: ansible
        ansible_ssh_private_key_file: "{{ ssh_identities_dir }}/ansible"
        ansible_python_interpreter: /usr/bin/python3
```

## Adding a new host

```bash
auberge host add my-vps 203.0.113.10 --user root --port 22
auberge ssh keygen --host my-vps --user ansible
auberge ansible bootstrap my-vps --ip 203.0.113.10
```

?> `auberge host add` (no args) detects `~/.ssh/config` and offers to import existing hosts.
