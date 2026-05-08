# Bichon

Email archiving service with continuous IMAP sync and full-text search. Docs: [github.com/rustmailer/bichon](https://github.com/rustmailer/bichon)

- **URL**: tailnet only — see [Tailnet-only apps](../../cli-reference/dns/set-all.md#tailnet-only-apps)
- **Port**: internal (Caddy proxy)
- **Data**: `/opt/bichon/data` (internal store), `/var/lib/bichon-archive` (EML mirror, backed up)

## Deploy

```bash
auberge deploy bichon
```

Bare-metal (no Docker). Requires Tailscale deployed first.

## Required config

| Key                          | Purpose                                   |
| ---------------------------- | ----------------------------------------- |
| `bichon_encryption_password` | Encrypts credentials and metadata DB      |
| `bichon_subdomain`           | Subdomain for HTTPS access                |
| `bichon_api_token`           | Bearer token for the hourly archive timer |

`bichon_api_token`: mint in Bichon's UI after first deploy, paste into `config.toml`, re-run.

!> `bichon_encryption_password` cannot be changed after first deploy. Changing it makes all encrypted data unreadable. The role enforces this: subsequent runs fail if the value differs.

## Notes

Default credentials: `admin` / `admin@bichon`. Change after first login.

**First-time setup:**

1. Add account via **Accounts → Add account** in the UI.
2. Reconcile folders:
   ```bash
   auberge bichon reconcile-folders --host <hostname> --apply
   ```
3. Seed the archive immediately:
   ```bash
   sudo systemctl start bichon-archive.service
   ```

**Backup**: `auberge backup create --apps bichon` rsyncs `/var/lib/bichon-archive` (not the internal store). The timer must have run at least once before the first backup. See [ADR-0006](https://github.com/sripwoud/auberge/blob/master/meta/adr/0006-bichon-archive-feeds-backup-recipe.md).

**Archived-then-expunge ordering** (do not skip steps):

1. Folders ticked in Bichon UI, `bichon.service` syncing.
2. `bichon-archive.timer` ran successfully — check `journalctl -u bichon-archive.service`.
3. `auberge backup sync` completed — archive is off-host.
4. Operator expunges manually (e.g. `himalaya`).

!> Check journal for errors before expunging — do not rely on archive mtime or message count alone. Unticked folders are not archived. Do not automate expunge on a cron.

Reference script: [`examples/bichon-expunge.sh`](https://github.com/sripwoud/auberge/blob/master/examples/bichon-expunge.sh) (prints the expunge command, does not run it).
