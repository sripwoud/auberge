# Bichon

Email archiving service with continuous IMAP sync and full-text search. Docs: [github.com/rustmailer/bichon](https://github.com/rustmailer/bichon)

- **URL**: tailnet only — see [Tailnet-only apps](cli-reference/dns/set-all.md#tailnet-only-apps)
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
3. Archive is off-host — `auberge backup sync`, then confirm it landed:
   ```bash
   auberge backup verify --app bichon
   ```
   Exit `0` means the newest offsite snapshot contains the archive and is younger than 24h. Anything else: stop, do not expunge. See [backup verify](cli-reference/backup/verify.md).
4. Operator expunges manually (e.g. `himalaya`).

!> Check journal for errors before expunging — do not rely on archive mtime or message count alone. Unticked folders are not archived. Do not automate expunge on a cron.

**Reference script**: [`examples/bichon-expunge.sh`](https://github.com/sripwoud/auberge/blob/master/examples/bichon-expunge.sh) turns the ordering above into five gates and, once they all pass, executes the expunge.

```bash
bash examples/bichon-expunge.sh --host <hostname> --account you@example.com
```

| Gate | Assertion                                                          |
| ---- | ------------------------------------------------------------------ |
| 1    | `auberge backup verify --app bichon` exits 0                       |
| 2    | last `bichon-archive.service` run succeeded, less than 3h ago      |
| 3    | archived `.eml` count >= IMAP count, scoped to `--folder`          |
| 4    | summary: exact himalaya commands, message count, snapshot evidence |
| 5    | operator types the folder name at a prompt                         |

Defaults: `--folder INBOX`, `--window-days 90`, `--archive-path /var/lib/bichon-archive`. On a TTY, a missing `--host` or `--account` is prompted for. Gate 3 also aborts if anything in the folder already carries the `\Deleted` flag, since the expunge would take it along.

Deletion is `himalaya flag add … deleted` followed by `himalaya folder expunge` — messages are removed in place, not moved to Trash, so mailbox quota is actually reclaimed.

!> The expunge needs an interactive TTY. There is no `--yes`/`--force`; `--no-input` and non-TTY stdin run every gate and then refuse to expunge. Per [ADR-0007](https://github.com/sripwoud/auberge/blob/master/meta/adr/0007-bichon-folder-reconcile-scope-and-silent-vs-loud.md) no unattended expunge path exists, and the script is not shipped in the `auberge` binary.
