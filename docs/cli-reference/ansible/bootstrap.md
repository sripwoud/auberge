# auberge ansible bootstrap

Bootstrap a fresh VPS with initial configuration

## Synopsis

```bash
auberge ansible bootstrap [OPTIONS] <HOST>
```

## Alias

`auberge a b`

## Description

Bootstraps a fresh VPS by running the bootstrap playbook with the bootstrap user (typically root). This is the first command to run on a new VPS before regular Ansible playbooks.

The bootstrap process:

1. Connects as bootstrap user (root) via password or SSH key
2. Creates ansible user with sudo privileges
3. Configures SSH hardening
4. Sets up firewall (UFW)
5. Changes SSH port to custom value

**Critical**: Ensure VPS provider firewall allows custom SSH port BEFORE running.

## Arguments

| Argument | Description                             |
| -------- | --------------------------------------- |
| HOST     | Host name from configuration (required) |

## Options

| Option      | Description                        | Default            |
| ----------- | ---------------------------------- | ------------------ |
| --port PORT | SSH port for initial connection    | 22                 |
| --ip IP     | IP address (required with --force) | Interactive prompt |
| -f, --force | Skip confirmation prompts          | false              |

## Examples

```bash
# Interactive bootstrap (prompts for IP)
auberge ansible bootstrap myserver

# Specify IP address
auberge ansible bootstrap myserver --ip 192.168.1.10

# Custom initial SSH port
auberge ansible bootstrap myserver --port 22222 --ip 192.168.1.10

# Non-interactive mode (requires --ip)
auberge ansible bootstrap myserver --ip 192.168.1.10 --force
```

## Prerequisites

Before running bootstrap:

1. **Add host to configuration**:
   ```bash
   auberge host add myserver 192.168.1.10 --user ansible
   ```

2. **Configure bootstrap user in inventory**:
   The host must have `bootstrap_user` variable set (typically "root")

3. **VPS provider firewall**:
   ```bash
   # Get your custom SSH port
   mise env | grep SSH_PORT

   # Add firewall rule in provider dashboard
   # Allow TCP on your SSH_PORT
   ```

4. **Have initial access**:
   - Password access to bootstrap user, OR
   - SSH key already authorized for bootstrap user

## Bootstrap Flow

```
1. Connect as root@host:22 (default port)
2. Create ansible user
3. Configure SSH hardening
4. Change SSH port to custom value
5. Configure UFW firewall
6. Verify ansible user access
```

After bootstrap completes, subsequent commands use the ansible user and custom SSH port.

## Troubleshooting

**Locked out after bootstrap**:

- Cause: Provider firewall not configured
- Fix: Add firewall rule in provider dashboard, reboot VPS

**IP validation fails**:

- Ensure valid IPv4 (e.g., 192.168.1.10) or IPv6 format

**Bootstrap playbook not found**:

- Ensure in project root with ansible/ directory
- Path should be: ansible/playbooks/bootstrap.yml

## Related Commands

- [auberge host add](../host/add.md) - Add host before bootstrapping
- [auberge ansible run](run.md) - Run playbooks after bootstrap
- [auberge ssh keygen](../ssh/keygen.md) - Generate SSH keys

## See Also

- [Bootstrap Guide](../../getting-started/bootstrap.md)
- [SSH Setup](../../getting-started/ssh-setup.md)
- [Firewall Configuration](../../core-concepts/security.md)
