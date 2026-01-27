# auberge ssh keygen

Generate SSH key pair for host authentication

## Synopsis

```bash
auberge ssh keygen [OPTIONS]
```

## Alias

`auberge ss k`

## Description

Generates an ED25519 SSH key pair for authenticating to a host. Keys are stored in ~/.ssh/identities/ with the naming convention `{user}_{host}`.

Keys are generated without a passphrase for automated access (required for Ansible and backup operations).

## Options

| Option          | Description                     | Default               |
| --------------- | ------------------------------- | --------------------- |
| -H, --host HOST | Target host                     | Interactive selection |
| -u, --user USER | User (ansible or your username) | ansible               |
| -f, --force     | Force overwrite existing key    | false                 |

## Examples

```bash
# Generate key (interactive host selection)
auberge ssh keygen

# Generate for specific host
auberge ssh keygen --host myserver

# Generate for custom user
auberge ssh keygen --host myserver --user deploy

# Force overwrite existing key
auberge ssh keygen --host myserver --force
```

## Key Generation Process

1. Creates ~/.ssh/identities/ directory if needed
2. Generates ED25519 key pair:
   - Private key: ~/.ssh/identities/{user}_{host}
   - Public key: ~/.ssh/identities/{user}_{host}.pub
3. Sets comment to `{user}@{host}`
4. No passphrase (empty)

## Output Example

```
Generating SSH key for ansible@myserver
✓ Generated key: ~/.ssh/identities/ansible_myserver
  Public key: ~/.ssh/identities/ansible_myserver.pub
```

## Key Format

**Private key** (ED25519):

```
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
...
-----END OPENSSH PRIVATE KEY-----
```

**Public key**:

```
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx... ansible@myserver
```

## Next Steps

After generating a key, you need to authorize it on the remote host:

**Option 1: Use auberge ssh add-key** (recommended):

```bash
auberge ssh add-key --host myserver
```

**Option 2: Manual authorization**:

```bash
ssh-copy-id -i ~/.ssh/identities/ansible_myserver ansible@host
```

## Why ED25519?

- **Faster**: Much faster than RSA
- **More secure**: 256-bit security (equivalent to 3072-bit RSA)
- **Smaller keys**: Shorter fingerprints and public keys
- **Modern**: Supported by all current OpenSSH versions

## Key Storage Location

Keys are stored separately from ~/.ssh/ to:

- Avoid cluttering main SSH directory
- Enable per-host key management
- Support multiple users per host

## Troubleshooting

**Key already exists**:

- Use `--force` to overwrite
- Or manually delete: `rm ~/.ssh/identities/ansible_myserver{,.pub}`

**Permission denied**:

- Ensure ~/.ssh/identities/ directory is writable
- Check disk space

## Related Commands

- [auberge ssh add-key](add-key.md) - Authorize key on remote host
- [auberge host add](../host/add.md) - Add host with SSH key

## See Also

- [SSH Setup](../../getting-started/ssh-setup.md)
- [Security Best Practices](../../core-concepts/security.md)
