# Cross-Host Migration

Restore a backup from one host to a different host with `--from-host`. Use cases: VPS provider migration, disaster recovery, seeding staging environments.

```bash
auberge backup restore latest --from-host old-vps --host new-vps
auberge backup restore latest --from-host old-vps --host new-vps --apps baikal,freshrss
auberge backup restore latest --from-host old-vps --host new-vps --dry-run
```

## Pre-flight checks

Before any data is moved, the CLI validates:

| Check                                         | Action on failure                                      |
| --------------------------------------------- | ------------------------------------------------------ |
| SSH reachable on target (10s timeout)         | Abort                                                  |
| Required systemd services installed on target | Abort — run `auberge ansible run --host new-vps` first |
| Free disk ≥ 120% of backup size               | Abort                                                  |

!> Cross-host restore prompts you to **type the target hostname** before proceeding. With `--yes`, a 3-second cancellable delay replaces the prompt.

## Emergency backup

Before overwriting data on the target, the CLI snapshots its current state:

```
✓ Emergency backup created: pre-migration-2026-01-23_15-30-00
  Location: ~/.local/share/auberge/backups/new-vps/{app}/2026-01-23_15-30-00
```

If the emergency backup fails, you're asked whether to continue.

## Post-restore

```bash
auberge ansible run --host new-vps --tags baikal,freshrss,navidrome  # regenerate host-specific config
auberge dns set-all --host new-vps                                   # repoint DNS
ssh user@new-vps 'systemctl status php*-fpm freshrss navidrome'      # verify services
curl -I https://cal.example.com                                       # verify SSL (Caddy auto-issues)
```

App-specific:

- **Baikal**: verify admin + DAV users in the web admin.
- **Navidrome**: rescan if music paths changed (web UI or restart service).
- **FreshRSS**: confirm feed refresh works.

## Common failures

| Error                                       | Fix                                                                                                           |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `Required service not found on target host` | `auberge ansible run --host new-vps`                                                                          |
| `Insufficient disk space`                   | Free space or resize VPS                                                                                      |
| Service won't start                         | `ssh user@host 'journalctl -u <service> -n 50'`. Permissions are auto-fixed; if still broken, re-run Ansible. |
