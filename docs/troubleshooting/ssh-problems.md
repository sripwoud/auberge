# SSH Problems

## Quick reference

`auberge ansible bootstrap` and `auberge ansible run --playbook bootstrap.yml` compare `~/.ssh/known_hosts` against the key the target offers. On a mismatch it prints the stale and offered fingerprints and asks whether to drop the entry; `--force` drops it and warns. Declining, or a non-TTY run without `--force`, aborts with the exact `ssh-keygen -R` to run. Entries are keyed by the host's name (`HostKeyAlias`), independent of its address or port.

| Symptom                                               | Likely cause                   | Fix                                                                                                                 |
| ----------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| `Connection refused`                                  | Wrong port after bootstrap     | `auberge config get ssh_port`; connect with `-p PORT`                                                               |
| `Permission denied (publickey)`                       | Wrong or missing key           | `ssh -i ~/.ssh/identities/vps/ansible ansible@vps-ip -p $SSH_PORT`                                                  |
| `Host key verification failed`                        | VPS reinstalled                | Bootstrap shows both fingerprints and offers removal; else run every `ssh-keygen -R` it prints                      |
| `Host key verification failed` right after an upgrade | Alias not yet in `known_hosts` | Self-repairing — the roster read migrates it before the connection; see below                                       |
| `Connection timed out`                                | Wrong IP or VPS offline        | `auberge host list`; check provider console                                                                         |
| Ansible hangs (ControlPersist)                        | Stale socket                   | `rm -rf ~/.ssh/ctl-*`                                                                                               |
| Unreachable at a `100.x.x.x` address                  | Stale or down tailnet route    | `auberge --via public host detect-tailscale-ip <name>`; see [Tailnet Transport](configuration/tailnet-transport.md) |

## `Host key verification failed` right after an upgrade

Every connection is checked under the host's **name** (`HostKeyAlias`), not its address, so `~/.ssh/known_hosts` needs an entry keyed by that name. Entries predating the change are keyed by address, and reading `hosts.toml` copies them onto the name.

Every command reads the roster before it connects, so a version carrying that migration repairs this within the same run — you should not see the symptom at all. On a version that predates it, run any `auberge host` subcommand once, or upgrade.

Confirm the entry landed:

```bash
ssh-keygen -F <host-name>   # exit 0 and one key line once migrated
```

Nothing is removed: the address-keyed entry stays where it is. If the name genuinely has no key to inherit — a host you have never connected to — connect once by name so the generated include's `accept-new` applies:

```bash
ssh <host-name> true
```

Conversely, dropping a host's key means dropping **every** spelling of it — the name and the address-keyed entries — or the next roster read copies the old key back onto the name. `auberge ansible bootstrap` prints the full list of `ssh-keygen -R` commands when it refuses a changed key; run all of them.

## After-bootstrap lockout

Bootstrap changes the SSH port and deploys keys. If you can't connect afterwards:

1. Access via provider VNC/serial console.
2. Diagnose:

   ```bash
   sudo systemctl status sshd
   sudo grep "^Port" /etc/ssh/sshd_config
   sudo ufw status
   ```

3. Reset to port 22 if needed:

   ```bash
   sudo sed -i 's/^Port.*/Port 22/' /etc/ssh/sshd_config
   sudo systemctl restart sshd
   sudo ufw allow 22/tcp && sudo ufw enable
   ```

4. Fix the root cause, then re-run bootstrap.

!> Configuring the provider firewall to allow the custom SSH port **before** running bootstrap prevents this lockout.

## Key permission errors

```bash
chmod 600 ~/.ssh/identities/vps/ansible
```

## Key not found after upgrade

Keys created before the `identities/{host}/{user}` layout still sit at the flat legacy path. Move them instead of generating new ones — a fresh key is not authorized on the remote:

```bash
mkdir -p ~/.ssh/identities/<host>
mv ~/.ssh/identities/<user>_<host> ~/.ssh/identities/<host>/<user>
mv ~/.ssh/identities/<user>_<host>.pub ~/.ssh/identities/<host>/<user>.pub
```

## Wrong ansible_user

After bootstrap the `ansible` user replaces `root`. Verify `ansible/inventory.yml`:

```yaml
ansible_user: ansible
```

## Debugging

```bash
ssh -vvv ansible@vps-ip        # verbose SSH
auberge ansible run --host vps -vvv  # verbose Ansible
ansible -i ansible/inventory.yml vps -m ping  # connectivity test
```
