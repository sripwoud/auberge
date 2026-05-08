# Ansible Errors

## Task failures

| Error                            | Cause                                | Fix                                                                                 |
| -------------------------------- | ------------------------------------ | ----------------------------------------------------------------------------------- |
| `UNREACHABLE`                    | SSH connectivity                     | See [SSH Problems](ssh-problems.md)                                                 |
| `Permission denied`              | ansible user lacks passwordless sudo | `ssh ansible@vps "sudo -n true"`; re-bootstrap if needed                            |
| `apt lock`                       | Concurrent apt process               | Wait 60s; or via console: `sudo killall apt apt-get && sudo rm /var/lib/dpkg/lock*` |
| `Package not found`              | Stale apt cache                      | `ssh ansible@vps "sudo apt update"`                                                 |
| `Undefined variable`             | Config key not set                   | `auberge config list`; `auberge config set KEY value`                               |
| `Unit not found`                 | Service file missing                 | `auberge ansible run --host vps --tags service-name`                                |
| `No such file or directory`      | Parent path absent                   | `ssh ansible@vps "sudo mkdir -p /path/to/dir"`                                      |
| `Connection timeout` downloading | VPS network issue                    | Retry (idempotent); check `ssh ansible@vps "ping -c 3 8.8.8.8"`                     |

## Service won't start

```bash
ssh ansible@vps "journalctl -u service-name -n 50"
ssh ansible@vps "sudo ss -tulpn | grep PORT"   # port conflict
auberge ansible run --host vps --tags service-name  # fix permissions/config
```

## Handler not running

Task must be marked `changed` for handler to fire. If config was already in the desired state, the handler is skipped.

```bash
ssh ansible@vps "sudo systemctl restart service-name"
```

## Idempotency: task always shows `changed`

Caused by `command`/`shell` modules without `changed_when`. Add `changed_when: false` for read-only commands, or switch to the appropriate module (`file`, `package`, `service`).

## Debugging

```bash
auberge ansible run --host vps -vvv          # verbose
auberge deploy --all --check --host vps      # dry run
auberge ansible run --host vps --tags tag    # isolate by tag
ansible-playbook -i ansible/inventory.yml ansible/playbooks/apps.yml -vvv  # direct
```

## Recovery

```bash
# Partially applied playbook — re-run (idempotent)
auberge ansible run --host vps --playbook playbooks/apps.yml

# Inconsistent state
auberge deploy --all --host vps
# or
auberge backup restore latest --host vps
```
