# Bichon

Email archiving service with continuous IMAP sync and full-text search. Docs: [github.com/rustmailer/bichon](https://github.com/rustmailer/bichon)

- **URL**: tailnet only — see [Tailnet-only apps](cli-reference/dns/set-all.md#tailnet-only-apps)
- **Port**: internal (Caddy proxy)
- **Data**: `/opt/bichon/data` (internal store), `/var/lib/bichon-archive` (EML mirror, backed up)
- **Timers**: `bichon-archive.timer` (hourly archive), `bichon-uidvalidity-watch.timer` (hourly [rebuild alert](#uidvalidity-rebuild-alert))

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

The archive holds one `.eml` per message (byte-exact), a `.meta.json` sidecar recording its folder and its `message_id`, and one `tags.json` per account mapping RFC 5322 `Message-ID` to tags. `tags.json` is rewritten in full on every archive run — tags are mutable, so they are snapshotted rather than captured once.

The `<envelope-id>` in a filename is storage, not identity: Bichon mints a fresh envelope identifier on re-import, so one message can occupy several `.eml` files. `message_id` is the message's canonicalized RFC 5322 `Message-ID`, read from the body; a body with no such header is keyed by its `sha256`. Count distinct `message_id`, never files:

```bash
# messages in the archive, not copies
ssh <hostname> "sudo find /var/lib/bichon-archive -name '*.meta.json' \
  -exec jq -r .message_id {} + | sort -u | wc -l"
```

Sidecars written before [ADR-0013](https://github.com/sripwoud/auberge/blob/master/meta/adr/0013-archive-message-identity-is-the-message-id.md) carry no `message_id`. The next `bichon-archive.service` run repairs every one of them and drops the inert `tags` field [ADR-0012](https://github.com/sripwoud/auberge/blob/master/meta/adr/0012-archive-splits-immutable-bodies-from-mutable-metadata.md) left behind; until it has, gate 3 of `bichon-expunge.sh` refuses to run.

A payload that is not a message is refused rather than archived — Bichon answers `200` with zero bytes when an envelope's blob store entry is empty, which `curl --fail` reads as success. One already in the archive is refetched by the envelope id in its filename on the next run ([ADR-0015](https://github.com/sripwoud/auberge/blob/master/meta/adr/0015-archive-publishes-a-body-only-if-it-is-a-message.md)). A body Bichon can no longer serve fails the unit on every run and is named in the journal; clearing it means deleting **both** the body and its sidecar, since a sidecar with no body fails the run too.

```bash
# bodies the archive should have refused (expect no output)
ssh <hostname> "sudo find /var/lib/bichon-archive -name '*.eml' -empty"
```

## UIDVALIDITY rebuild alert

When an Upstream Mailbox changes a folder's `UIDVALIDITY`, Bichon deletes that folder's envelopes and blobs from the internal store and refetches. Mail already expunged upstream **cannot be refetched** — its searchability and tags are gone until you replay the archive. Bichon logs this at info level and carries on.

`bichon-uidvalidity-watch.timer` reads `bichon.service`'s journal hourly and records every occurrence. It shows up as a failed unit:

```bash
systemctl --failed
systemctl status bichon-uidvalidity-watch.service   # names the folder and when
```

The alert is **latched**: it is reported on every run until you acknowledge it, because systemd would otherwise clear the failed state on the next tick. The full record is `/var/lib/bichon-uidvalidity-watch/rebuilds.log`.

| Exit | Meaning                                  |
| ---- | ---------------------------------------- |
| 0    | no rebuild recorded                      |
| 1    | a rebuild is recorded and unacknowledged |
| 2    | the journal could not be read            |

To respond: restore the affected folder per **Restore ordering** below, then acknowledge — deleting the file is the acknowledgement.

```bash
sudo rm /var/lib/bichon-uidvalidity-watch/rebuilds.log
sudo systemctl start bichon-uidvalidity-watch.service   # exits 0, clears the failed state
```

!> Acknowledge only after restoring. Nothing else records that a purge happened, so deleting the latch without restoring discards the only notice you get. The first run after deploy reports rebuilds already in the retained journal — that is deliberate, not a false alarm.

See [ADR-0014](https://github.com/sripwoud/auberge/blob/master/meta/adr/0014-uidvalidity-rebuild-alert-is-a-latched-failing-unit.md). The alert is passive by design: it carries no push channel, so it reaches you when you look at the Host or Cockpit.

**Restore ordering** (do not skip steps):

1. `auberge deploy bichon`, then add the account via **Accounts → Add account** — restore does not create accounts.
2. Let folders sync, then reconcile: `auberge bichon reconcile-folders --host <hostname> --apply`. Bichon only imports into folders it already knows.
3. Restore `/var/lib/bichon-archive` from restic if it is not still on the Host.
4. Run [`examples/bichon-restore.sh`](https://github.com/sripwoud/auberge/blob/master/examples/bichon-restore.sh):
   ```bash
   BICHON_API_TOKEN=… bash examples/bichon-restore.sh \
     --host http://127.0.0.1:15630 --account you@example.com
   ```

The script imports every `.eml` into the folder its sidecar records, then replays `tags.json` onto the imported messages with `action: Add`, so re-running the tag replay never removes anything. `--dry-run` prints the folder and tag-set plan without touching the API. It reports `imported / tagged / skipped / failed` counts and exits non-zero on any failure.

!> Re-running a completed restore duplicates messages — Bichon's import mints a new envelope id per message. And messages that arrived without a `Message-ID` header carry a synthetic one that does not survive re-import: their tags are logged and counted as skipped, the bodies restore fine.

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

Defaults: `--folder INBOX`, `--window-days 90`, `--archive-path /var/lib/bichon-archive`. On a TTY, a missing `--host` is chosen from `auberge host list` and a missing `--account` from `himalaya account list`. Gate 3 also aborts if anything in the folder already carries the `\Deleted` flag, since the expunge would take it along.

A preflight runs before the gates, resolving one value at a time: tools on `PATH` (all missing ones reported at once), `auberge backup verify` present, himalaya holding at least one configured account, ssh reachability, then the Email Archive root.

`--account` must be the mailbox email address. himalaya account names are arbitrary labels, but bichon keys archive directories by email (`sanitize_email` in `bichon-archive.sh.j2`) and the script passes one value to both — so the himalaya account has to be named after the address. The menu offers only accounts present on both sides; a mismatched `--account` is rejected by name before gate 1.

!> The ssh user must be in the `bichon` group. The archive is `0750 bichon:bichon` and gate 3 reads `.meta.json` sidecars without sudo, so without it the gate cannot read anything. Grant with `ssh <host> 'sudo usermod -aG bichon $(whoami)'`, then reconnect.

Deletion is `himalaya flag add … deleted` followed by `himalaya folder expunge` — messages are removed in place, not moved to Trash, so mailbox quota is actually reclaimed.

!> The expunge needs an interactive TTY. There is no `--yes`/`--force`; `--no-input` and non-TTY stdin run every gate and then refuse to expunge. Per [ADR-0007](https://github.com/sripwoud/auberge/blob/master/meta/adr/0007-auberge-folder-reconcile-scope.md) no unattended expunge path exists, and the script is not shipped in the `auberge` binary.

**Expunge Sweep**: `--sweep` walks every eligible (account, Synced Folder) pair on the Host with one window, instead of one operator-chosen pair (ADR-0007, amendment 2026-08-03). Excludes `--account`/`--folder`.

```bash
bash examples/bichon-expunge.sh --sweep --host <hostname> --window-days 90
```

Gates 1–2 run once for the Host; gate 3 runs per pair, and a failing pair is skipped as a named **finding** instead of aborting the rest (coverage gap, pre-existing `\Deleted` mail, unkeyed sidecar, a listing the tools could not produce). An empty window, reconcile drift, or an account missing on either the himalaya or the archive side is a benign skip. Gates 4–5 become one summary table — every pair, its status, its count — and two typed checkpoints:

1. **scope** — type the Bichon Host name
2. **magnitude** — type the grand message total from the summary

Exit codes: `0` clean sweep, `1` at least one finding (read the report), `2` operational error or an empty target set. A folder left with `\Deleted` mail by a mid-sweep failure is refused by the next run's pre-existing-flag check, so a partial sweep cannot compound silently.

`--sweep --no-input` is a cron-safe fleet-wide coverage verification: every gate runs, nothing is expunged, exit `1` flags that a finding needs attention.
