# Ansible Errors

## Task failures

| Error                            | Cause                                | Fix                                                                                 |
| -------------------------------- | ------------------------------------ | ----------------------------------------------------------------------------------- |
| `UNREACHABLE`                    | SSH connectivity                     | See [SSH Problems](troubleshooting/ssh-problems.md)                                 |
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

## Ingress gate failed

Every play that can restart caddy ends with an `ingress_gate` assertion. It fails when caddy is not `active (running)`, or when vhosts exist and nothing listens on `:443` — this deploy took ingress down for every app on the host.

The cause is almost always a vhost that `bind`s an address the host does not own: caddy exits, systemd auto-restarts it, and `systemctl restart` still exits 0.

```bash
ssh ansible@vps "systemctl status caddy; journalctl -u caddy -n 30"
ssh ansible@vps "grep -r bind /etc/caddy/sites"   # every bind must be an address the host owns
ssh ansible@vps "ip -4 -brief addr"               # what it actually owns
```

A stale `<app>_tailscale_ip` in `~/.config/auberge/config.toml` is the usual source: config keys reach ansible as extra-vars, which outrank the Tailscale IP the role detects at run time. Correct or delete the key, then redeploy.

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
