# Hosts vs Inventory

Auberge uses two separate host configuration systems for different purposes. Understanding the difference is crucial for proper usage.

## Quick Comparison

| Feature                | hosts.toml                     | inventory.yml                 |
| ---------------------- | ------------------------------ | ----------------------------- |
| **Location**           | `~/.config/auberge/hosts.toml` | `ansible/inventory.yml`       |
| **Format**             | TOML                           | YAML                          |
| **Used by**            | Backup/restore, SSH commands   | Ansible playbooks             |
| **Version controlled** | No (user-specific)             | Yes (shared)                  |
| **Management**         | `auberge host` commands        | Manual editing                |
| **Purpose**            | Runtime host information       | Deployment targets            |
| **SSH keys**           | Explicit file paths            | Environment variable patterns |

## hosts.toml (User Configuration)

### Purpose

Personal host registry for backup and SSH operations. Each user maintains their own list of VPS hosts.

### Location

```bash
~/.config/auberge/hosts.toml
```

### Format

```toml
[[hosts]]
name = "my-vps"
address = "203.0.113.10"
user = "sripwoud"
port = 22
ssh_key = "~/.ssh/identities/sripwoud_my-vps"
tags = ["production"]
description = "Main production VPS"

[[hosts]]
name = "staging"
address = "10.0.0.1"
user = "ansible"
port = 2222
ssh_key = "~/.ssh/staging_key"
tags = ["staging", "testing"]
```

### Management

Use CLI commands to manage hosts:

```bash
# Add host interactively
auberge host add

# Add host non-interactively
auberge host add my-vps 203.0.113.10 --user ansible --port 22

# List all hosts
auberge host list

# Show host details
auberge host show my-vps

# Edit host
auberge host edit my-vps

# Remove host
auberge host remove my-vps
```

### When Used

- `auberge backup create` - Needs SSH connection details
- `auberge backup restore` - Needs SSH connection details
- `auberge backup export-opml` - Needs SSH connection details
- `auberge backup import-opml` - Needs SSH connection details
- `auberge ssh keygen` - Generates keys for host

### SSH Key Resolution

hosts.toml supports explicit SSH key configuration:

```toml
[[hosts]]
name = "my-vps"
ssh_key = "/home/user/.ssh/custom_key"  # Explicit path
```

If `ssh_key` is not set:

1. Derives default: `~/.ssh/identities/{user}_{hostname}`
2. If default doesn't exist, prompts for key selection

See [SSH Keys](configuration/ssh-keys.md) for details.

### Not Version Controlled

This file is **user-specific** and should not be committed to git:

- Different team members may have different VPS hosts
- SSH keys and ports may differ per user
- Personal testing/staging environments

## inventory.yml (Ansible Configuration)

### Purpose

Defines deployment targets for Ansible playbooks. Shared across team members via version control.

### Location

```bash
ansible/inventory.yml
```

### Format

```yaml
all:
  children:
    vps:
      hosts:
        auberge:
          ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
          ansible_port: "{{ lookup('env', 'SSH_PORT') }}"
          bootstrap_user: root
        staging:
          ansible_host: "{{ lookup('env', 'STAGING_HOST') }}"
          ansible_port: 22
          bootstrap_user: root
      vars:
        ansible_user: ansible
        ansible_ssh_private_key_file: "{{ lookup('env', 'HOME') }}/.ssh/identities/ansible_{{ inventory_hostname }}"
        admin_ssh_private_key_file: "{{ lookup('env', 'HOME') }}/.ssh/identities/{{ admin_user_name }}_{{ inventory_hostname }}"
        ansible_python_interpreter: /usr/bin/python3
        ansible_ssh_pipelining: true
```

### Management

Manually edit the YAML file:

```bash
# Edit inventory
vim ansible/inventory.yml

# Validate inventory syntax
ansible-inventory -i ansible/inventory.yml --list
```

### When Used

- `auberge ansible run` - Selects host from inventory
- `auberge ansible bootstrap` - Needs bootstrap_user
- `auberge ansible check` - Dry-run against inventory hosts

### Environment Variable Integration

inventory.yml uses environment variables for sensitive data:

```yaml
ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
ansible_port: "{{ lookup('env', 'SSH_PORT') }}"
```

This allows:

- Shared inventory file in version control
- Personal environment variables for actual IPs/ports
- Different values for dev/staging/prod

### Version Controlled

This file **is committed** to git:

- Defines infrastructure topology
- Shared deployment configuration
- Team collaboration

## Usage Scenarios

### Scenario 1: Personal Backup

**Goal:** Back up your personal VPS

**Configuration needed:** hosts.toml only

```bash
# Add host
auberge host add my-vps 203.0.113.10

# Create backup
auberge backup create --host my-vps
```

**Why:** Backup operations use hosts.toml for SSH connection details.

### Scenario 2: Team Deployment

**Goal:** Deploy applications to shared infrastructure

**Configuration needed:** inventory.yml + environment variables

```bash
# Set environment variables
export AUBERGE_HOST="203.0.113.10"
export SSH_PORT="2222"

# Deploy
auberge ansible run --host auberge --playbook playbooks/apps.yml
```

**Why:** Ansible playbooks use inventory.yml for deployment targets.

### Scenario 3: Both Backup and Deploy

**Goal:** Full operations (backup + deploy) on same VPS

**Configuration needed:** Both files

**hosts.toml:**

```toml
[[hosts]]
name = "my-vps"
address = "203.0.113.10"
user = "sripwoud"
port = 2222
```

**inventory.yml:**

```yaml
hosts:
  auberge:
    ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
    ansible_port: "{{ lookup('env', 'SSH_PORT') }}"
```

**Environment:**

```bash
export AUBERGE_HOST="203.0.113.10"
export SSH_PORT="2222"
```

**Operations:**

```bash
# Deploy using inventory.yml
auberge ansible run --host auberge

# Backup using hosts.toml
auberge backup create --host my-vps
```

## Migration Between Systems

### Import from SSH Config

When adding a host interactively, you can import from `~/.ssh/config`:

```bash
auberge host add

# If ~/.ssh/config exists, shows:
Found 3 host(s) in ~/.ssh/config

? Import from SSH config or enter manually?:
  > Enter manually
    ansible-old (203.0.113.10)
    myserver (10.0.0.1)
```

This auto-fills host details from existing SSH configuration.

### Export to Inventory

No automatic export exists. Manually sync if needed:

```bash
# View hosts.toml
auberge host list

# Manually update ansible/inventory.yml based on output
vim ansible/inventory.yml
```

## Priority and Fallback

When selecting hosts for Ansible operations, the CLI checks:

1. `~/.config/auberge/hosts.toml` (if exists and not empty)
2. `ansible/inventory.yml` (fallback for developers)
3. Environment variables (legacy support)

For backup operations:

1. `~/.config/auberge/hosts.toml` only (inventory not used)

## Best Practices

### For End Users (Installed via `cargo install`)

Use **hosts.toml** exclusively:

```bash
# Add all your VPS hosts
auberge host add production 203.0.113.10
auberge host add staging 10.0.0.1

# Use for everything
auberge backup create --host production
auberge ansible run --host production
```

### For Developers (Cloned Repository)

Use **both**:

- **inventory.yml**: Version-controlled deployment targets
- **hosts.toml**: Personal backup configuration (optional)

```bash
# Deploy using inventory
auberge ansible run --host auberge

# Backup using hosts (if configured)
auberge backup create --host auberge
```

### For Teams

**Shared:** inventory.yml in repository

**Personal:** Each member has their own hosts.toml

**Secrets:** Use environment variables referenced in inventory.yml

```yaml
# inventory.yml (shared)
ansible_host: "{{ lookup('env', 'PROD_HOST') }}"

# Each team member's .env (personal)
PROD_HOST="203.0.113.10"  # Actual IP
```

## Related Pages

- [SSH Keys](configuration/ssh-keys.md) - SSH key configuration
- [Hosts Configuration](configuration/hosts.md) - hosts.toml management
- [Ansible Inventory](configuration/ansible-inventory.md) - inventory.yml structure
- [Environment Variables](configuration/environment-variables.md) - Variable reference
