# auberge backup restore

Restore application data from a backup. Alias: `auberge b r`.

```bash
auberge backup restore [OPTIONS] [BACKUP_ID]
```

`BACKUP_ID` is a timestamp (`YYYY-MM-DD_HH-MM-SS`) or `latest`. Omit to be prompted (newest first).

## Arguments

| Argument    | Description                                 |
| ----------- | ------------------------------------------- |
| `BACKUP_ID` | Timestamp or `latest` (omit to be prompted) |

## Options

| Option                   | Description                             | Default                           |
| ------------------------ | --------------------------------------- | --------------------------------- |
| `-H, --host HOST`        | Target host                             | Interactive                       |
| `-F, --from-host HOST`   | Source host (cross-host migration)      | Same as target                    |
| `-a, --apps APPS`        | Apps to restore (comma-separated)       | All in backup                     |
| `-k, --ssh-key PATH`     | SSH private key                         | `~/.ssh/identities/{user}_{host}` |
| `-n, --dry-run`          | Preview without restoring               | false                             |
| `-y, --yes`              | Skip confirmation prompt                | false                             |
| `--skip-playbook-unsafe` | Skip Ansible playbook run after restore | false                             |

## Examples

```bash
auberge backup restore latest --host myserver              # restore all apps
auberge backup restore latest --host myserver --apps baikal,freshrss
auberge backup restore 2024-01-27_14-30-00 --host myserver
auberge backup restore latest --host newserver --from-host oldserver  # migration
auberge backup restore latest --host myserver --dry-run
```

## Gotchas

!> Cross-host migration runs a pre-flight check (SSH, services, disk ≥120% of backup size), creates an emergency backup tagged `pre-migration-{timestamp}` on the target, then requires you to retype the target hostname to confirm. After restore, Ansible playbooks run automatically to fix ownership and permissions. Use `--skip-playbook-unsafe` only as a last resort; if skipped, run manually: `cd ansible && ansible-playbook playbooks/apps.yml --tags <apps>`.

- **SSH/service failures**: verify key and run `auberge ansible run` to install missing apps first.
- **Insufficient disk**: free space or exclude large apps with `--apps`.
- **Services fail after restore**: check ownership with `ls -la /var/lib/<app>`, then rerun playbooks.
