# Backup/Restore Issues

## Backup creation

| Error                 | Cause                                 | Fix                                                                                                           |
| --------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `"SSH key not found"` | Key not generated                     | `auberge ssh keygen --host my-vps --user ansible`                                                             |
| `"No such directory"` | Service not deployed                  | `auberge ansible run --host my-vps --tags baikal`, then retry                                                 |
| `"Permission denied"` | ansible user can't read service files | `ssh ansible@vps "sudo -n ls /opt/baikal/Specific"`; re-run `auberge ansible run --host my-vps --tags baikal` |

### Backup hangs

Kill stale control sockets and retry:

```bash
rm -rf ~/.ssh/ctl-*
auberge backup create --host my-vps
```

?> If backup is interrupted (network drop, SIGKILL), stopped services restart automatically via a remote failsafe timer (30-minute timeout). Verify: `ssh ansible@vps "systemctl list-timers | grep auberge-backup-failsafe"`.

## Restore

| Error                           | Cause                          | Fix                                                                            |
| ------------------------------- | ------------------------------ | ------------------------------------------------------------------------------ |
| `"No backups found for host"`   | No backup exists               | `auberge backup list`; create one first                                        |
| `"Service not found on target"` | App not deployed on target VPS | Deploy first: `auberge ansible run --host new-vps --tags baikal`               |
| `"Insufficient disk space"`     | Target VPS full                | `ssh ansible@vps "df -h"`; use `--apps baikal,freshrss` to restore selectively |

### Service won't start after restore

```bash
ssh ansible@vps "journalctl -u php*-fpm -n 50"
auberge ansible run --host vps --tags baikal   # re-applies permissions/config
ssh ansible@vps "sudo systemctl restart php*-fpm"
```

Causes: incorrect file permissions, config incompatible with new host, port conflicts.

## Cross-host restore

- `--yes` skips hostname confirmation but adds a 3-second safety delay.
- Pre-flight validation failure (emergency backup fails): no `--skip` flag yet — fix the underlying issue and retry.

## OPML (FreshRSS)

| Error                          | Fix                                                 |
| ------------------------------ | --------------------------------------------------- |
| `"FreshRSS not installed"`     | `auberge ansible run --host my-vps --tags freshrss` |
| `"Cannot connect to FreshRSS"` | `ssh ansible@vps "sudo systemctl restart freshrss"` |

## Performance

Music is excluded by default. Include only when needed:

```bash
auberge backup create --host vps --include-music
```

For OOM during backup, split by app:

```bash
auberge backup create --host vps --apps baikal
auberge backup create --host vps --apps freshrss
```

## Data integrity

```bash
ls -lah ~/.local/share/auberge/backups/my-vps/baikal/latest/
auberge backup restore latest --host my-vps --dry-run
```
