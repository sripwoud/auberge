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

?> A service probe that never reached the target aborts as a transport failure rather than as a missing service, so `not found` is not a network diagnosis. The one case it cannot tell apart is a remote command that exits 255 having printed nothing.

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

Apps declaring `restore_advice` in their Backup Recipe — Navidrome and FreshRSS — have `backup restore` print their note at the end of a cross-host run, so it is not repeated here. Baikal declares none.

## When the tailnet route is down

A host with `prefer_tailnet = true` is reached at its `tailscale_ip` and nothing falls back to the public address — see [Tailnet Transport](configuration/tailnet-transport.md). Headscale runs on `auberge`, so the tailnet route to the backup target depends on a service that target hosts, and a disaster recovery is exactly when that matters.

Prefix any command with `--via public` to route it over the declared public address for that run:

```bash
auberge --via public backup restore latest --from-host old-vps --host new-vps
auberge --via public backup sync --host auberge
auberge --via public ansible run --host new-vps
auberge --via public host detect-tailscale-ip auberge   # after the tailnet is back, if the address moved
```

`--via public` applies to SSH, rsync and Ansible together, so there is no half-migrated route to reason about. It never rewrites `~/.ssh/config.d/auberge.conf`, so interactive `ssh <name>` is unaffected — for a shell during an outage, use the address directly:

```bash
ssh -p "$(auberge config get ssh_port)" -i ~/.ssh/identities/auberge/ansible ansible@203.0.113.10
```

!> A `--via` given to a command that connects to no host (`host list`, `config get`) exits non-zero saying it changed nothing. The command still ran. Drop the flag rather than reading it as a failure.

?> Do not "fix" an outage by editing `prefer_tailnet` out of `hosts.toml`. That is a fleet-wide, persistent change made under pressure, and it also regenerates the ssh include. `--via public` is scoped to one command and leaves no state behind.

## Common failures

| Error                                       | Fix                                                                                                           |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `Required service not found on target host` | `auberge ansible run --host new-vps`                                                                          |
| `Insufficient disk space`                   | Free space or resize VPS                                                                                      |
| Service won't start                         | `ssh user@host 'journalctl -u <service> -n 50'`. Permissions are auto-fixed; if still broken, re-run Ansible. |
