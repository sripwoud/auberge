# auberge ssh add-key

Authorize SSH public key on remote host

## Synopsis

```bash
auberge ssh add-key [OPTIONS]
```

## Description

Adds (authorizes) an SSH public key to a remote host's ~/.ssh/authorized_keys file. Requires existing SSH access to the target host.

This command:

1. Connects to remote host with specified key
2. Creates ~/.ssh directory if needed
3. Appends public key to authorized_keys
4. Sets correct permissions

## Options

| Option                 | Description                      | Default               |
| ---------------------- | -------------------------------- | --------------------- |
| -H, --host HOST        | Target host                      | Interactive selection |
| -c, --connect-with KEY | SSH private key to connect with  | Interactive selection |
| -a, --authorize KEY    | Public key to authorize          | Interactive selection |
| -u, --user USER        | Remote user to authorize key for | ansible               |
| -y, --yes              | Skip confirmation prompt         | false                 |

## Examples

```bash
# Interactive mode (recommended)
auberge ssh add-key

# Authorize default key for host
auberge ssh add-key --host myserver

# Authorize specific key
auberge ssh add-key --host myserver --authorize ~/.ssh/id_ed25519.pub

# Connect with password, authorize new key
auberge ssh add-key --host myserver --connect-with ~/.ssh/id_rsa --authorize ~/.ssh/identities/ansible_myserver.pub

# For different user
auberge ssh add-key --host myserver --user deploy

# Skip confirmation
auberge ssh add-key --host myserver --yes
```

## Interactive Flow

```
1. Select host
2. Select key to connect with (from ~/.ssh/identities/ and ~/.ssh/)
3. Select public key to authorize
4. Show summary and confirm
5. Execute
```

## Process Details

**Connection key** (--connect-with):

- Key that currently has access to remote host
- Can be password-based if first-time setup
- Scans:
  - ~/.ssh/identities/
  - ~/.ssh/id_* keys

**Key to authorize** (--authorize):

- Public key (.pub file) to add to authorized_keys
- Will enable future access with corresponding private key
- Scans:
  - ~/.ssh/identities/*.pub
  - ~/.ssh/*.pub

## Output Example

```
Add SSH Key
Host: myserver (192.168.1.10:22)
Remote user: ansible
Connection key: ~/.ssh/identities/ansible_oldkey
Key to authorize: ~/.ssh/identities/ansible_myserver.pub
Public key preview: ssh-ed25519 AAAAC3NzaC1... ansible@myserver

Authorize this key on the remote host? [y/N]: y

Adding key to remote host
✓ Key authorized successfully on ansible@myserver
```

## Use Cases

**Initial setup**: Authorize ansible key after VPS creation

```bash
# Connect with password or provider key, authorize ansible key
auberge ssh add-key --host newserver --authorize ~/.ssh/identities/ansible_newserver.pub
```

**Key rotation**: Replace old key with new one

```bash
# Generate new key
auberge ssh keygen --host myserver

# Connect with old key, authorize new key
auberge ssh add-key --host myserver --connect-with ~/.ssh/identities/ansible_myserver_old --authorize ~/.ssh/identities/ansible_myserver.pub
```

**Multi-user**: Authorize additional users

```bash
# Authorize deploy user key
auberge ssh add-key --host myserver --user deploy --authorize ~/.ssh/identities/deploy_myserver.pub
```

## Bootstrap Scenario

For fresh VPS bootstrap:

```bash
# 1. Generate ansible key locally
auberge ssh keygen --host myserver --user ansible

# 2. Add host (bootstrap user is root)
auberge host add myserver 192.168.1.10 --user ansible

# 3. Authorize ansible key (connect as root first time)
# You'll be prompted for root password
ssh-copy-id -i ~/.ssh/identities/ansible_myserver.pub root@192.168.1.10

# 4. Run bootstrap (creates ansible user)
auberge ansible bootstrap myserver --ip 192.168.1.10

# 5. Future commands use ansible key automatically
```

## Troubleshooting

**Permission denied**:

- Verify connection key exists and is correct
- Check remote user exists
- Try with password: `ssh-copy-id` directly

**Key already authorized**:

- Not a problem - key will be added again (duplicates are ignored by SSH)

**~/.ssh not created**:

- Command creates it automatically with correct permissions (700)

**Wrong permissions after adding**:

- Command sets:
  - ~/.ssh: 700
  - ~/.ssh/authorized_keys: 600

## Related Commands

- [auberge ssh keygen](keygen.md) - Generate new SSH key
- [auberge ansible bootstrap](../ansible/bootstrap.md) - Bootstrap with SSH setup

## See Also

- [SSH Setup Guide](../../getting-started/ssh-setup.md)
- [Security](../../core-concepts/security.md)
