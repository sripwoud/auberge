# SSH Problems

## Quick reference

| Symptom                         | Likely cause               | Fix                                                                |
| ------------------------------- | -------------------------- | ------------------------------------------------------------------ |
| `Connection refused`            | Wrong port after bootstrap | `auberge config get ssh_port`; connect with `-p PORT`              |
| `Permission denied (publickey)` | Wrong or missing key       | `ssh -i ~/.ssh/identities/ansible_vps ansible@vps-ip -p $SSH_PORT` |
| `Host key verification failed`  | VPS reinstalled            | `ssh-keygen -R vps-ip`                                             |
| `Connection timed out`          | Wrong IP or VPS offline    | `auberge host list`; check provider console                        |
| Ansible hangs (ControlPersist)  | Stale socket               | `rm -rf ~/.ssh/ctl-*`                                              |

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
chmod 600 ~/.ssh/identities/ansible_vps
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
