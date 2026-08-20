# auberge host rename

Rename a host: remote hostname, hosts.toml entry, and key directory

## Synopsis

```bash
auberge host rename <OLD> <NEW> [--yes]
# Alias: auberge h mv
```

## Description

Renames a host everywhere its name is load-bearing, in one command:

1. **Remote**: `hostnamectl set-hostname <NEW>` and the matching `/etc/hosts` entries (via sudo over SSH).
2. **Local**: the `hosts.toml` entry, the key directory `~/.ssh/identities/<OLD>` → `<NEW>`, and the configured `ssh_key` path when it points inside that directory. A custom `ssh_key` outside the derived tree is left untouched — file and path. The generated `~/.ssh/config.d/auberge.conf` is rewritten, so `ssh <NEW>` works immediately (see [SSH keys](../../configuration/ssh-keys.md)).

Preflight bails before touching anything: `<OLD>` must exist in `hosts.toml`, `<NEW>` must not, the key directories must not collide, and SSH to the host must succeed.

Remote steps run first, so a failure there aborts with zero local change. Every step is idempotent: after a partial failure, rerunning the same command is the recovery path (see ADR-0024).

## Arguments

| Argument | Description       |
| -------- | ----------------- |
| OLD      | Current host name |
| NEW      | New host name     |

## Options

| Option        | Description       |
| ------------- | ----------------- |
| `-y`, `--yes` | Skip confirmation |

## Not done by this command

- **tailscale**: the host re-advertises itself under the new name and releases the old tailnet name.
- **restic**: snapshots group by host name — the old lineage freezes at the rename and the new name starts a fresh one. Never rewrite snapshot tags.

## Examples

```bash
# Rename with confirmation prompt
auberge host rename auberge vieille-auberge

# Non-interactive (ceremony scripts)
auberge host rename auberge vieille-auberge --yes
```
